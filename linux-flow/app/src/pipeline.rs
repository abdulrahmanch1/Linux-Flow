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

// Re-focusing the target window before injection normally completes in well
// under a millisecond; a full second only ever elapsed when the window was
// unresponsive, freezing the app on the release→inject path. Bound the wait
// tightly: a healthy focus switch finishes far inside this window, and a stuck
// one no longer stalls injection for a whole second.
const FOCUS_SYNC_TIMEOUT: Duration = Duration::from_millis(300);

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

/// Tears down the running hotkey backend and starts a fresh one bound to
/// `new_key`. The old manager is fully stopped (and its resources awaited)
/// before the new one starts, so no duplicate events or leaked sessions occur.
/// If the new key cannot be bound, the previous key is restored and the user is
/// notified rather than left with no working hotkey.
async fn reload_hotkey(
    hotkey: &mut HotkeyManager,
    new_key: &str,
    events: &mpsc::Sender<PipelineEvent>,
) {
    hotkey.shutdown().await;
    match HotkeyManager::start(new_key, None) {
        Ok(fresh) => {
            *hotkey = fresh;
            tracing::info!("pipeline: hotkey reloaded to {new_key}");
        }
        Err(e) => {
            tracing::warn!("pipeline: new hotkey '{new_key}' failed ({e}), restoring previous");
            let old_key = hotkey.key().to_owned();
            match HotkeyManager::start(&old_key, None) {
                Ok(restored) => *hotkey = restored,
                Err(e2) => tracing::error!("pipeline: could not restore hotkey: {e2}"),
            }
            send_event(
                events,
                PipelineEvent::Error(format!(
                    "Hotkey '{new_key}' is not supported — kept the previous one."
                )),
            )
            .await;
        }
    }
}

/// Returns the X11 window currently holding input focus, or `None` on a
/// Wayland-native target. `getwindowfocus` uses XGetInputFocus — it works for
/// XWayland apps on GNOME Wayland where `getactivewindow` (`_NET_ACTIVE_WINDOW`)
/// fails. The root window (id 1) is excluded.
async fn capture_focused_window(display: DisplayServer) -> Option<u64> {
    if display == DisplayServer::X11 || std::env::var_os("DISPLAY").is_some() {
        tokio::process::Command::new("xdotool")
            .arg("getwindowfocus")
            .output()
            .await
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&id| id > 1)
    } else {
        None
    }
}

/// Maps a finished ASR transcription error into a user-facing message.
fn map_asr_error(e: AsrError) -> String {
    match e {
        AsrError::ConnectTimeout | AsrError::StreamTimeout => {
            "Could not connect to Soniox. Check your connection and API key.".to_owned()
        }
        AsrError::Unauthorized => {
            "Soniox API key is invalid. Open Settings to update it.".to_owned()
        }
        AsrError::ServerError { code: 402, .. } => {
            "Soniox balance exhausted — add funds at soniox.com.".to_owned()
        }
        AsrError::ServerError { code, message } => format!("Soniox error {code}: {message}"),
        _ => "Speech recognition failed. Please try again.".to_owned(),
    }
}

/// Re-focuses the captured window just before injection, so apps that ignore
/// synthetic events on unfocused windows still receive the text. Best-effort:
/// any failure is logged and injection proceeds via the captured `--window`.
async fn refocus_window(win: u64) {
    match tokio::time::timeout(
        FOCUS_SYNC_TIMEOUT,
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

/// Normalizes the trailing break before injection. Single-line mode strips any
/// dangling newline so it never lands as a lone Enter (which would submit/run
/// the text prematurely). Splitting mode appends exactly one newline so the
/// final segment also sends, matching the earlier segments.
fn normalize_for_injection(text: &str, message_splitting: bool) -> Zeroizing<String> {
    let trimmed = text.trim_end_matches(['\n', '\r', ' ', '\t']);
    if message_splitting {
        Zeroizing::new(format!("{trimmed}\n"))
    } else {
        Zeroizing::new(trimmed.to_owned())
    }
}

pub struct Pipeline {
    config: Config,
    display: DisplayServer,
    llm: LlmClient,
    soniox_key: Zeroizing<String>,
    or_key: Zeroizing<String>,
    ipc: IpcClient,
    config_rx: mpsc::Receiver<Config>,
    _config_watcher: Option<notify::RecommendedWatcher>,
}

impl Pipeline {
    pub async fn new(config: Config) -> Result<Self, PipelineError> {
        let llm = LlmClient::new().map_err(|e| PipelineError::LlmInit(e.to_string()))?;
        // Load both keys over a single Secret Service session so cold start pays
        // one D-Bus connect + unlock rather than one round trip per key.
        let keys = secrets::resolve_keys(&[
            ("SONIOX_API_KEY", "soniox"),
            ("OPENROUTER_API_KEY", "openrouter"),
        ])
        .await
        .map_err(|e| PipelineError::KeyLoad(e.to_string()))?;
        let [soniox_key, or_key] = keys
            .try_into()
            .map_err(|_| PipelineError::KeyLoad("expected exactly two resolved keys".to_owned()))?;
        let ipc = IpcClient::new();
        let (config_tx, config_rx) = mpsc::channel::<Config>(4);
        let _config_watcher: Option<notify::RecommendedWatcher> = (|| {
            let path = Config::config_path().ok()?;
            let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if res.is_ok() {
                    if let Ok(fresh) = Config::load() {
                        // notify runs this in its own (non-async) thread.
                        let _ = config_tx.blocking_send(fresh);
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

    /// Rewrites `raw` through the LLM and maps every failure to a user event.
    /// Returns the text to inject, or `None` when an error was already surfaced
    /// and the caller should move on to the next press. `raw` is consumed and
    /// zeroized on every path; on an empty LLM reply it falls back to the raw
    /// transcript and emits `LlmFallback`.
    async fn process_llm(
        &mut self,
        mut raw: Zeroizing<String>,
        events: &mpsc::Sender<PipelineEvent>,
    ) -> Option<Zeroizing<String>> {
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| self.config.llm.model.clone());
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
                    events,
                    PipelineEvent::Error(
                        "OpenRouter API key is invalid. Open Settings to update it.".into(),
                    ),
                )
                .await;
                return None;
            }
            Err(LlmError::Http(429)) => {
                raw.zeroize();
                send_event(
                    events,
                    PipelineEvent::Error("Rate limit reached — try again in a moment.".into()),
                )
                .await;
                return None;
            }
            Err(LlmError::Http(503)) => {
                raw.zeroize();
                send_event(
                    events,
                    PipelineEvent::Error("OpenRouter is temporarily unavailable.".into()),
                )
                .await;
                return None;
            }
            Err(e) => {
                tracing::warn!("pipeline: llm error: {e}");
                raw.zeroize();
                send_event(
                    events,
                    PipelineEvent::Error(
                        "Could not reach OpenRouter. Check your connection.".into(),
                    ),
                )
                .await;
                return None;
            }
        };
        raw.zeroize();
        if text.trim().is_empty() {
            send_event(
                events,
                PipelineEvent::Error("No speech detected. Try speaking more clearly.".into()),
            )
            .await;
            text.zeroize();
            return None;
        }
        if fallback {
            send_event(events, PipelineEvent::LlmFallback).await;
        }
        Some(text)
    }

    pub async fn run(&mut self, mut hotkey: HotkeyManager, events: mpsc::Sender<PipelineEvent>) {
        loop {
            // Idle: watch the hotkey and live config changes together. Applying
            // a hotkey change here — not mid-session — means a fresh key takes
            // effect on the next press without restarting the app, while an
            // in-progress recording is never cut short.
            let pressed = loop {
                tokio::select! {
                    ev = hotkey.next() => match ev {
                        Some(HotkeyEvent::Pressed) => break true,
                        Some(HotkeyEvent::Released) => continue,
                        None => break false,
                    },
                    Some(fresh) = self.config_rx.recv() => {
                        let hotkey_changed =
                            fresh.general.hotkey != self.config.general.hotkey;
                        self.config = fresh;
                        if hotkey_changed {
                            reload_hotkey(
                                &mut hotkey,
                                &self.config.general.hotkey,
                                &events,
                            )
                            .await;
                        }
                    }
                }
            };
            if !pressed {
                break;
            }

            let focused_win = capture_focused_window(self.display).await;

            let mut audio = AudioCapture::new();
            let (pcm_rx, audio_err_rx, ready_rx) = match audio.start() {
                Ok(triple) => triple,
                Err(e) => {
                    send_event(
                        &events,
                        PipelineEvent::Error(format!("Audio capture failed: {e}")),
                    )
                    .await;
                    continue;
                }
            };
            // Wait for PipeWire to deliver its first capture callback. A real
            // failure (init/connect/stream/pod error) drops the ready sender as
            // capture_loop returns, resolving this immediately into the err
            // branch below — so this timeout only bounds a healthy-but-slow cold
            // start (busy box, VM, first press after launch), not error
            // reporting. It is off the release→inject latency path, so a
            // generous window costs nothing on the happy path while sparing a
            // valid press from being rejected on a slow system.
            if !matches!(
                tokio::time::timeout(Duration::from_secs(5), ready_rx).await,
                Ok(Ok(()))
            ) {
                let msg = if let Ok(e) = audio_err_rx.try_recv() {
                    format!("Audio capture failed: {e}")
                } else {
                    "Audio device didn't start in time — please try again.".into()
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
                tracing::warn!(
                    "pipeline: daemon pre-connect failed ({e}), will retry at inject time"
                );
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
            // Surface the underlying cause to journald before collapsing it into
            // a generic user message: a silently swallowed ASR failure leaves no
            // diagnostic trail at all in a normal autostart run.
            let asr_result = asr_joined
                .map_err(|e| {
                    tracing::warn!("asr task failed to join: {e}");
                    "Speech recognition failed. Please try again.".to_owned()
                })
                .and_then(|r| {
                    r.map_err(|e| {
                        tracing::warn!("asr failed: {e}");
                        map_asr_error(e)
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

            let Some(mut text) = self.process_llm(raw, &events).await else {
                continue;
            };

            tracing::info!("pipeline: inject start");
            if let Some(win) = focused_win {
                refocus_window(win).await;
            }
            let to_inject = normalize_for_injection(&text, self.config.llm.message_splitting);
            match self.ipc.inject_text(&to_inject, focused_win).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_strips_trailing_break() {
        // A dangling newline must never survive — it would land as a lone Enter
        // and submit the text prematurely.
        let out = normalize_for_injection("hello world\n", false);
        assert_eq!(&*out, "hello world");
    }

    #[test]
    fn single_line_strips_mixed_trailing_whitespace() {
        let out = normalize_for_injection("done \t\r\n ", false);
        assert_eq!(&*out, "done");
    }

    #[test]
    fn single_line_keeps_interior_newlines() {
        // Only the trailing break is stripped; interior structure is preserved.
        let out = normalize_for_injection("line a\nline b\n", false);
        assert_eq!(&*out, "line a\nline b");
    }

    #[test]
    fn splitting_appends_exactly_one_newline() {
        let out = normalize_for_injection("send me", true);
        assert_eq!(&*out, "send me\n");
    }

    #[test]
    fn splitting_collapses_multiple_trailing_breaks_to_one() {
        let out = normalize_for_injection("msg\n\n\n", true);
        assert_eq!(&*out, "msg\n");
    }

    #[test]
    fn map_asr_error_timeouts_share_connectivity_message() {
        let connect = map_asr_error(AsrError::ConnectTimeout);
        let stream = map_asr_error(AsrError::StreamTimeout);
        assert_eq!(connect, stream);
        assert!(connect.contains("connect to Soniox"));
    }

    #[test]
    fn map_asr_error_unauthorized_points_to_settings() {
        let msg = map_asr_error(AsrError::Unauthorized);
        assert!(msg.contains("invalid"));
        assert!(msg.contains("Settings"));
    }

    #[test]
    fn map_asr_error_402_is_balance_message() {
        let msg = map_asr_error(AsrError::ServerError {
            code: 402,
            message: "ignored".to_owned(),
        });
        assert!(msg.contains("balance"));
    }

    #[test]
    fn map_asr_error_other_server_error_includes_code_and_message() {
        let msg = map_asr_error(AsrError::ServerError {
            code: 500,
            message: "boom".to_owned(),
        });
        assert!(msg.contains("500"));
        assert!(msg.contains("boom"));
    }

    #[test]
    fn map_asr_error_fallback_is_generic() {
        let msg = map_asr_error(AsrError::Stream("low level detail".to_owned()));
        // Internal details must not leak into the user-facing message.
        assert!(!msg.contains("low level detail"));
        assert!(msg.contains("Speech recognition failed"));
    }
}
