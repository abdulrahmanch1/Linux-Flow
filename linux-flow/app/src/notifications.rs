use gtk4::glib;
use libadwaita as adw;
use std::sync::mpsc as stdmpsc;
use tokio::sync::mpsc;

use crate::{
    pipeline::PipelineEvent,
    tray::{TrayHandle, TrayState},
};

/// Drives tray state and notifications from the pipeline event stream.
///
/// When `toast_tx` is Some (a GTK window is open), error messages are forwarded
/// to the GTK main thread for display as AdwToast. Otherwise libnotify is used.
pub async fn handle_events(
    mut rx: mpsc::Receiver<PipelineEvent>,
    tray: TrayHandle,
    toast_tx: Option<stdmpsc::SyncSender<String>>,
) {
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
                match &toast_tx {
                    Some(tx) => { let _ = tx.send(msg.clone()); }
                    None => notify_desktop(msg).await,
                }
            }
        }
    }
}

/// Wire a channel receiver to show AdwToast on the given overlay.
/// Call this once when opening an application window. Polls every 50 ms.
pub fn connect_toast_overlay(rx: stdmpsc::Receiver<String>, overlay: adw::ToastOverlay) {
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        while let Ok(msg) = rx.try_recv() {
            overlay.add_toast(adw::Toast::new(&msg));
        }
        glib::ControlFlow::Continue
    });
}

async fn notify_desktop(body: &str) {
    let _ = tokio::process::Command::new("/usr/bin/notify-send")
        .args(["--app-name=linux-flow", "--icon=dialog-warning", "linux-flow", body])
        .status()
        .await;
}
