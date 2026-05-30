use crate::{
    asr::{self, AsrError},
    audio::AudioCapture,
    hotkey::{HotkeyEvent, HotkeyManager},
    inject::IpcClient,
    llm::{LlmClient, LlmError},
    secrets,
};
use linux_flow_shared::{config::Config, display::DisplayServer};
use notify::{RecursiveMode, Watcher};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone)]
pub enum PipelineEvent {
    RecordingStarted,
    ProcessingStarted,
    TextInjected,
    Error(String),
    LlmFallback,
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("LLM client init: {0}")]
    LlmInit(String),
    #[error("API key load: {0}")]
    KeyLoad(String),
}

/// Sends a pipeline event, logging a warning if the receiver has been dropped.
async fn send_event(tx: &mpsc::Sender<PipelineEvent>, event: PipelineEvent) {
    if tx.send(event).await.is_err() {
        tracing::warn!("pipeline: event channel closed — UI feedback lost");
    }
}

pub struct Pipeline {
    config: Config,
    display: DisplayServer,
    llm: LlmClient,
    soniox_key: Zeroizing<String>,
    or_key: Zeroizing<String>,
    ipc: IpcClient,
    config_rx: std::sync::mpsc::Receiver<Config>,
    _config_watcher: Option<notify::RecommendedWatcher>,
}

impl Pipeline {
    pub async fn new(config: Config) -> Result<Self, PipelineError> {
        let llm = LlmClient::new().map_err(|e| PipelineError::LlmInit(e.to_string()))?;
        let soniox_key = secrets::resolve_key("SONIOX_API_KEY", "soniox")
            .await
            .map_err(|e| PipelineError::KeyLoad(e.to_string()))?;
        let or_key = secrets::resolve_key("OPENROUTER_API_KEY", "openrouter")
            .await
            .map_err(|e| PipelineError::KeyLoad(e.to_string()))?;
        let ipc = IpcClient::new();
        let (config_tx, config_rx) = std::sync::mpsc::channel::<Config>();
        let _config_watcher: Option<notify::RecommendedWatcher> = (|| {
            let path = Config::config_path().ok()?;
            let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if res.is_ok() {
                    if let Ok(fresh) = Config::load() {
                        let _ = config_tx.send(fresh);
                    }
                }
            })
            .ok()?;
            w.watch(&path, RecursiveMode::NonRecursive).ok()?;
            Some(w)
        })();
        Ok(Self {
            config,
            display: linux_flow_shared::display::detect(),
            llm,
            soniox_key,
            or_key,
            ipc,
            config_rx,
            _config_watcher,
        })
    }

    pub async fn run(&mut self, mut hotkey: HotkeyManager, events: mpsc::Sender<PipelineEvent>) {
        loop {
            if !matches!(hotkey.next().await, Some(HotkeyEvent::Pressed)) {
                break;
            }

            // `getwindowfocus` uses XGetInputFocus — works for XWayland apps on
            // GNOME Wayland where `getactivewindow` (_NET_ACTIVE_WINDOW) fails.
            let focused_win: Option<u64> =
                if self.display == DisplayServer::X11
                    || std::env::var_os("DISPLAY").is_some()
                {
                    tokio::process::Command::new("xdotool")
                        .arg("getwindowfocus")
                        .output()
                        .await
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .filter(|&id| id > 1) // exclude root window (id=1)
                } else {
                    None
                };

            while let Ok(fresh) = self.config_rx.try_recv() {
                self.config = fresh;
            }
            let mut audio = AudioCapture::new();
            let (pcm_rx, audio_err_rx, ready_rx) = match audio.start() {
                Ok(triple) => triple,
                Err(e) => {
                    send_event(&events, PipelineEvent::Error(format!("Audio capture failed: {e}")))
                        .await;
                    continue;
                }
            };
            if !matches!(
                tokio::time::timeout(Duration::from_secs(2), ready_rx).await,
                Ok(Ok(()))
            ) {
                let msg = if let Ok(e) = audio_err_rx.try_recv() {
                    format!("Audio capture failed: {e}")
                } else {
                    "Audio device not responding.".into()
                };
                send_event(&events, PipelineEvent::Error(msg)).await;
                continue;
            }
            send_event(&events, PipelineEvent::RecordingStarted).await;
            tracing::info!("pipeline: recording started");

            let soniox_key = self.soniox_key.clone();
            let language = self.config.soniox.language.clone();
            let asr =
                tokio::spawn(async move { asr::transcribe(soniox_key, &language, pcm_rx).await });

            let released = loop {
                match hotkey.next().await {
                    Some(HotkeyEvent::Released) => break true,
                    Some(HotkeyEvent::Pressed) => {}
                    None => break false,
                }
            };
            tracing::info!("pipeline: hotkey released={released}");
            // Fire-and-forget: the stop flag tells PipeWire to quit its loop,
            // and closing pcm_rx already signals EOS to the ASR task. Joining
            // the thread is not on the hot path.
            tokio::task::spawn_blocking(move || audio.stop());
            tracing::info!("pipeline: audio stop requested");
            if let Err(e) = self.ipc.ensure_connected().await {
                tracing::warn!("pipeline: daemon pre-connect failed ({e}), will retry at inject time");
            }
            if !released {
                asr.abort();
                send_event(
                    &events,
                    PipelineEvent::Error("Hotkey service disconnected.".into()),
                )
                .await;
                break;
            }
            if let Ok(e) = audio_err_rx.try_recv() {
                send_event(
                    &events,
                    PipelineEvent::Error(format!("Audio capture failed: {e}")),
                )
                .await;
                continue;
            }
            send_event(&events, PipelineEvent::ProcessingStarted).await;
            tracing::info!("pipeline: asr waiting");

            let asr_joined = asr.await;
            tracing::info!(
                "pipeline: asr done ok={}",
                asr_joined.as_ref().map(|r| r.is_ok()).unwrap_or(false)
            );
            if let Ok(Err(AsrError::Unauthorized)) = &asr_joined {
                let _ = secrets::resolve_key("SONIOX_API_KEY", "soniox")
                    .await
                    .map(|k| self.soniox_key = k);
            }
            let asr_result = asr_joined
                .map_err(|_| "Speech recognition failed. Please try again.".to_owned())
                .and_then(|r| {
                    r.map_err(|e| match e {
                        AsrError::ConnectTimeout | AsrError::StreamTimeout => {
                            "Could not connect to Soniox. Check your connection and API key."
                                .to_owned()
                        }
                        AsrError::Unauthorized => {
                            "Soniox API key is invalid. Open Settings to update it.".to_owned()
                        }
                        AsrError::ServerError { code: 402, .. } => {
                            "Soniox balance exhausted — add funds at soniox.com.".to_owned()
                        }
                        AsrError::ServerError { code, message } => {
                            format!("Soniox error {code}: {message}")
                        }
                        _ => "Speech recognition failed. Please try again.".to_owned(),
                    })
                });
            let mut raw = match asr_result {
                Ok(t) => t,
                Err(msg) => {
                    send_event(&events, PipelineEvent::Error(msg)).await;
                    continue;
                }
            };

            // Guard against silence: a press with no speech yields an empty
            // transcript. Feeding that to the LLM makes weak models hallucinate
            // a large unrelated reply, which then gets injected. Never call the
            // LLM on empty input.
            if raw.trim().is_empty() {
                raw.zeroize();
                send_event(
                    &events,
                    PipelineEvent::Error("No speech detected. Try speaking more clearly.".into()),
                )
                .await;
                continue;
            }

            let model =
                std::env::var("LLM_MODEL").unwrap_or_else(|_| self.config.llm.model.clone());
            tracing::info!("pipeline: llm start");
            let llm_res = self
                .llm
                .rewrite(
                    self.or_key.clone(),
                    &model,
                    &raw,
                    self.config.llm.message_splitting,
                )
                .await;
            tracing::info!("pipeline: llm done ok={}", llm_res.is_ok());
            let (mut text, fallback) = match llm_res {
                Ok(t) => (t, false),
                Err(LlmError::EmptyResponse) => (
                    std::mem::replace(&mut raw, Zeroizing::new(String::new())),
                    true,
                ),
                Err(LlmError::Unauthorized) => {
                    raw.zeroize();
                    let _ = secrets::resolve_key("OPENROUTER_API_KEY", "openrouter")
                        .await
                        .map(|k| self.or_key = k);
                    send_event(
                        &events,
                        PipelineEvent::Error(
                            "OpenRouter API key is invalid. Open Settings to update it.".into(),
                        ),
                    )
                    .await;
                    continue;
                }
                Err(LlmError::Http(429)) => {
                    raw.zeroize();
                    send_event(
                        &events,
                        PipelineEvent::Error(
                            "Rate limit reached — try again in a moment.".into(),
                        ),
                    )
                    .await;
                    continue;
                }
                Err(LlmError::Http(503)) => {
                    raw.zeroize();
                    send_event(
                        &events,
                        PipelineEvent::Error(
                            "OpenRouter is temporarily unavailable.".into(),
                        ),
                    )
                    .await;
                    continue;
                }
                Err(e) => {
                    tracing::warn!("pipeline: llm error: {e}");
                    raw.zeroize();
                    send_event(
                        &events,
                        PipelineEvent::Error(
                            "Could not reach OpenRouter. Check your connection.".into(),
                        ),
                    )
                    .await;
                    continue;
                }
            };
            raw.zeroize();
            if text.trim().is_empty() {
                send_event(
                    &events,
                    PipelineEvent::Error(
                        "No speech detected. Try speaking more clearly.".into(),
                    ),
                )
                .await;
                text.zeroize();
                continue;
            }
            if fallback {
                send_event(&events, PipelineEvent::LlmFallback).await;
            }

            tracing::info!("pipeline: inject start");
            if let Some(win) = focused_win {
                match tokio::time::timeout(
                    Duration::from_secs(1),
                    tokio::process::Command::new("xdotool")
                        .args(["windowfocus", "--sync", &win.to_string()])
                        .status(),
                )
                .await
                {
                    Ok(Ok(s)) if s.success() => {}
                    Ok(Ok(s)) => tracing::warn!("pipeline: windowfocus failed: exit={:?}", s.code()),
                    Ok(Err(e)) => tracing::warn!("pipeline: windowfocus error: {e}"),
                    Err(_) => tracing::warn!("pipeline: windowfocus timed out for window {win}"),
                }
            }
            // Normalize the trailing break. Single-line mode strips any dangling
            // newline so it never lands as a lone Enter (which would submit/run
            // the text prematurely). Splitting mode appends exactly one newline so
            // the final segment also sends, matching the earlier segments.
            let trimmed = text.trim_end_matches(['\n', '\r', ' ', '\t']);
            let with_break;
            let to_inject = if self.config.llm.message_splitting {
                with_break = Zeroizing::new(format!("{trimmed}\n"));
                with_break.as_str()
            } else {
                trimmed
            };
            match self.ipc.inject_text(to_inject, focused_win).await {
                Ok(()) => {
                    send_event(&events, PipelineEvent::TextInjected).await;
                    tracing::info!("pipeline: inject ok");
                }
                Err(msg) => {
                    tracing::warn!("pipeline: inject err: {msg}");
                    send_event(&events, PipelineEvent::Error(msg)).await;
                }
            }
            text.zeroize();
            tracing::info!("pipeline: session complete — waiting for next press");
        }
    }
}
