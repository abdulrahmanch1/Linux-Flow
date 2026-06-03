use std::time::Duration;
use tokio::sync::mpsc;

use crate::{
    pipeline::PipelineEvent,
    tray::{TrayHandle, TrayState},
};

/// Drives tray state and notifications from the pipeline event stream.
/// Errors are surfaced as desktop notifications via notify-send.
pub async fn handle_events(mut rx: mpsc::Receiver<PipelineEvent>, tray: TrayHandle) {
    while let Some(event) = rx.recv().await {
        match event {
            PipelineEvent::RecordingStarted => {
                tray.set_state(TrayState::Recording).await;
            }
            PipelineEvent::ProcessingStarted => {
                tray.set_state(TrayState::Processing).await;
            }
            PipelineEvent::TextInjected | PipelineEvent::LlmFallback => {
                tray.set_state(TrayState::Idle).await;
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
    let ok = tokio::process::Command::new("notify-send")
        .args([
            "--app-name=linux-flow",
            "--icon=dialog-warning",
            "linux-flow",
            body,
        ])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("linux-flow: {body}");
    }
}
