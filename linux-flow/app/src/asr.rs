use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rustls::{ClientConfig, RootCertStore};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message, Connector};
use zeroize::{Zeroize, Zeroizing};

const SONIOX_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_TIMEOUT: Duration = Duration::from_secs(30);

static TLS_CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();

fn tls_connector() -> Connector {
    let config = TLS_CONFIG.get_or_init(|| {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    });
    Connector::Rustls(Arc::clone(config))
}

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("connection timed out (3 s)")]
    ConnectTimeout,
    #[error("stream timed out after EOS (30 s)")]
    StreamTimeout,
    #[error("invalid Soniox API key (401)")]
    Unauthorized,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("Soniox server error {code}: {message}")]
    ServerError { code: u32, message: String },
    #[error("websocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct SonioxToken {
    text: String,
    is_final: bool,
}

#[derive(Deserialize)]
struct SonioxResponse {
    #[serde(default)]
    tokens: Vec<SonioxToken>,
    #[serde(default)]
    finished: bool,
    #[serde(default)]
    error_code: u32,
    #[serde(default)]
    error_message: String,
}

/// Connects to Soniox, streams PCM audio from `audio_rx`, sends EOS when the
/// channel closes (hotkey released), then returns the final transcript.
///
/// Timeouts: 3 s to establish the connection; 30 s from EOS to `finished`.
/// The shared TLS config enables session resumption on subsequent calls.
pub async fn transcribe(
    mut api_key: Zeroizing<String>,
    language: &str,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) -> Result<Zeroizing<String>, AsrError> {
    let connector = tls_connector();
    let (ws, _) = timeout(
        CONNECT_TIMEOUT,
        connect_async_tls_with_config(SONIOX_URL, None, false, Some(connector)),
    )
    .await
    .map_err(|_| AsrError::ConnectTimeout)?
    .map_err(AsrError::WebSocket)?;

    let (mut sink, stream) = ws.split();

    let hints: Vec<&str> = if language == "auto" {
        vec![]
    } else {
        vec![language]
    };
    let mut config_str = Zeroizing::new(
        json!({
            "api_key": *api_key,
            "model": "stt-rt-v4",
            "audio_format": "pcm_s16le",
            "sample_rate": 16000,
            "num_channels": 1,
            "enable_language_identification": true,
            "language_hints": hints,
        })
        .to_string(),
    );
    api_key.zeroize();
    // Take the String out (leaving config_str empty) so no extra copy of the key exists.
    let to_send = std::mem::take(&mut *config_str);
    config_str.zeroize();
    sink.send(Message::Text(to_send)).await?;

    let read_task = tokio::spawn(async move {
        let mut stream = stream;
        let mut transcript = Zeroizing::new(String::new());
        while let Some(msg) = stream.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(_) => continue,
                Err(e) => return Err(AsrError::WebSocket(e)),
            };
            let resp: SonioxResponse = serde_json::from_str(&text)?;
            tracing::info!(
                "asr: msg error={} finished={} tokens={}",
                resp.error_code,
                resp.finished,
                resp.tokens.len()
            );
            if resp.error_code == 401 {
                return Err(AsrError::Unauthorized);
            }
            if resp.error_code != 0 {
                return Err(AsrError::ServerError {
                    code: resp.error_code,
                    message: resp.error_message,
                });
            }
            for token in resp.tokens {
                if token.is_final {
                    transcript.push_str(&token.text);
                }
            }
            if resp.finished {
                return Ok(transcript);
            }
        }
        Err(AsrError::Stream(
            "connection closed before finished signal".into(),
        ))
    });

    while let Some(chunk) = audio_rx.recv().await {
        if let Err(e) = sink.send(Message::Binary(chunk)).await {
            read_task.abort();
            return Err(AsrError::WebSocket(e));
        }
    }

    if let Err(e) = sink.send(Message::Text(String::new())).await {
        read_task.abort();
        return Err(AsrError::WebSocket(e));
    }
    tracing::info!("asr: EOS sent — waiting up to 30 s for finished");
    let abort = read_task.abort_handle();

    let result = timeout(STREAM_TIMEOUT, read_task)
        .await
        .map_err(|_| {
            abort.abort();
            AsrError::StreamTimeout
        })?
        .map_err(|e| AsrError::Stream(e.to_string()))?;
    drop(sink);
    result
}
