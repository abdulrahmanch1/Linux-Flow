use std::time::Duration;
use tokio::sync::mpsc;

use crate::{
    pipeline::PipelineEvent,
    tray::{TrayHandle, TrayState},
};

/// Drives tray state and notifications from the pipeline event stream.
/// Errors — and the raw-transcript fallback — are surfaced as desktop
/// notifications via notify-send.
pub async fn handle_events(mut rx: mpsc::Receiver<PipelineEvent>, tray: TrayHandle) {
    while let Some(event) = rx.recv().await {
        match event {
            PipelineEvent::RecordingStarted => {
                tray.set_state(TrayState::Recording).await;
            }
            PipelineEvent::ProcessingStarted => {
                tray.set_state(TrayState::Processing).await;
            }
            PipelineEvent::TextInjected => {
                tray.set_state(TrayState::Idle).await;
            }
            PipelineEvent::LlmFallback => {
                // The text was injected, so this is not an error state — but the
                // user should know it skipped the rewrite and got the raw words.
                tray.set_state(TrayState::Idle).await;
                notify_desktop("Inserted the raw transcript — couldn't polish it this time.").await;
            }
            PipelineEvent::Error(ref msg) => {
                tray.set_state(TrayState::Error).await;
                notify_desktop(msg).await;
                // Reset to Idle after 3 s so the tray doesn't stay red indefinitely.
                let tray_reset = tray.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    tray_reset.set_state(TrayState::Idle).await;
                });
            }
        }
    }
}

async fn notify_desktop(body: &str) {
    let body = body.to_owned();
    let _ = tokio::task::spawn_blocking(move || notify_user_blocking(&body)).await;
}

/// Fires a one-off desktop notification from a synchronous context (startup,
/// settings open) where no tokio runtime drives the event loop. Falls back to
/// stderr when `notify-send` is missing or fails, so the message is never lost.
pub fn notify_user_blocking(body: &str) {
    let ok = std::process::Command::new("notify-send")
        .args([
            "--app-name=linux-flow",
            "--icon=dialog-warning",
            "linux-flow",
            body,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("linux-flow: {body}");
    }
}
