use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use zeroize::{Zeroize, Zeroizing};

const SONIOX_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("connection timed out (3 s)")]
    ConnectTimeout,
    #[error("stream timed out after EOS (8 s)")]
    StreamTimeout,
    #[error("invalid Soniox API key (401)")]
    Unauthorized,
    #[error("stream error: {0}")]
    Stream(String),
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
/// Timeouts: 3 s to establish the connection; 8 s from EOS to `finished`.
pub async fn transcribe(
    mut api_key: Zeroizing<String>,
    language: &str,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) -> Result<Zeroizing<String>, AsrError> {
    let (ws, _) = timeout(CONNECT_TIMEOUT, connect_async(SONIOX_URL))
        .await
        .map_err(|_| AsrError::ConnectTimeout)?
        .map_err(AsrError::WebSocket)?;

    let (mut sink, stream) = ws.split();

    // Build and send the config frame, then zeroize the api_key immediately.
    let hints: Vec<&str> = if language == "auto" { vec![] } else { vec![language] };
    let mut config_str = Zeroizing::new(json!({
        "api_key": *api_key,
        "model": "stt-rt-v4",
        "audio_format": "pcm_s16le",
        "sample_rate": 16000,
        "num_channels": 1,
        "enable_language_identification": true,
        "language_hints": hints,
    })
    .to_string());
    api_key.zeroize();
    let msg_text = (*config_str).clone();
    config_str.zeroize();
    sink.send(Message::Text(msg_text)).await?;

    // Spawn the reader. It accumulates is_final tokens and resolves on `finished`.
    // Auth / stream errors surface here and are returned after EOS.
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
            if resp.error_code == 401 {
                return Err(AsrError::Unauthorized);
            }
            if resp.error_code != 0 {
                return Err(AsrError::Stream(resp.error_message));
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
        Err(AsrError::Stream("connection closed before finished signal".into()))
    });

    // Forward PCM chunks until the audio channel closes (hotkey released).
    while let Some(chunk) = audio_rx.recv().await {
        if let Err(e) = sink.send(Message::Binary(chunk)).await {
            read_task.abort();
            return Err(AsrError::WebSocket(e));
        }
    }

    // EOS: empty binary frame signals end of audio to Soniox.
    if let Err(e) = sink.send(Message::Binary(vec![])).await {
        read_task.abort();
        return Err(AsrError::WebSocket(e));
    }

    // Wait for the final transcript. 8 s timeout starts here (after EOS).
    timeout(STREAM_TIMEOUT, read_task)
        .await
        .map_err(|_| AsrError::StreamTimeout)?
        .map_err(|e| AsrError::Stream(e.to_string()))?
}
