use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rustls::{ClientConfig, RootCertStore};
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message, Connector};
use zeroize::{Zeroize, Zeroizing};

const SONIOX_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
// After EOS we bound *inactivity*, not total time: a healthy stream keeps
// delivering messages until `finished` (sub-second normally), so this only
// trips when the connection has genuinely stalled — freeing the UI fast
// without cutting off a slow-but-progressing finalize.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

static TLS_CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();

fn tls_connector() -> Connector {
    let config = TLS_CONFIG.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
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
    #[error("stream went idle after EOS (no response for 5 s)")]
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
/// Timeouts: 3 s to establish the connection; after EOS, the wait for
/// `finished` is bounded by inactivity (5 s with no message), not a fixed cap.
/// The shared TLS config enables session resumption on subsequent calls.
pub async fn transcribe(
    mut api_key: Zeroizing<String>,
    language: &str,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) -> Result<Zeroizing<String>, AsrError> {
    let connector = tls_connector();
    let (ws, _) = timeout(
        CONNECT_TIMEOUT,
        connect_async_tls_with_config(SONIOX_URL, None, true, Some(connector)),
    )
    .await
    .map_err(|_| AsrError::ConnectTimeout)?
    .map_err(AsrError::WebSocket)?;

    let (mut sink, stream) = ws.split();

    let hints_json = if language == "auto" {
        "[]".to_owned()
    } else {
        format!("[\"{language}\"]")
    };
    // Build JSON manually to avoid serde_json::Value cloning the API key.
    let mut config_str = Zeroizing::new(format!(
        concat!(
            "{{",
            "\"api_key\":\"{}\",",
            "\"model\":\"stt-rt-v4\",",
            "\"audio_format\":\"pcm_s16le\",",
            "\"sample_rate\":16000,",
            "\"num_channels\":1,",
            "\"enable_language_identification\":true,",
            "\"language_hints\":{}",
            "}}"
        ),
        *api_key, hints_json,
    ));
    api_key.zeroize();
    let to_send = std::mem::take(&mut *config_str);
    config_str.zeroize();
    sink.send(Message::Text(to_send)).await?;

    // A bounded liveness channel: read_task pings it on every message so the
    // post-EOS wait below can tell "slow but progressing" from "stalled".
    let (activity_tx, mut activity_rx) = mpsc::channel::<()>(1);

    let read_task = tokio::spawn(async move {
        let mut stream = stream;
        let mut transcript = Zeroizing::new(String::new());
        while let Some(msg) = stream.next().await {
            // Non-blocking liveness ping; a full channel already means "alive".
            let _ = activity_tx.try_send(());
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(_) => continue,
                Err(e) => return Err(AsrError::WebSocket(e)),
            };
            let resp: SonioxResponse = serde_json::from_str(&text)?;
            tracing::debug!(
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
    tracing::info!("asr: EOS sent — waiting for finished (idle-bounded)");
    let abort = read_task.abort_handle();

    // Wait for the transcript, resetting an idle window on every message. A
    // stalled connection (no message for STREAM_IDLE_TIMEOUT) aborts promptly
    // instead of freezing the UI on a long fixed cap, while a slow finalize
    // that keeps sending messages is never cut off.
    let mut read_task = read_task;
    loop {
        tokio::select! {
            joined = &mut read_task => {
                break joined.map_err(|e| AsrError::Stream(e.to_string()))?;
            }
            _ = activity_rx.recv() => {
                // A message arrived (or the task ended); restart the idle window.
                continue;
            }
            _ = tokio::time::sleep(STREAM_IDLE_TIMEOUT) => {
                abort.abort();
                break Err(AsrError::StreamTimeout);
            }
        }
    }
}
