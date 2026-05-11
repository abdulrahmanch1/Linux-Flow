use crate::{
    pipeline::PipelineEvent,
    tray::{TrayHandle, TrayState},
};
use gtk4::glib;
use libadwaita as adw;
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use tokio::sync::mpsc;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait FdoNotifications {
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

async fn notify_dbus(summary: &str, body: &str) {
    let Ok(conn) = zbus::Connection::session().await else {
        return;
    };
    let Ok(proxy) = FdoNotificationsProxy::new(&conn).await else {
        return;
    };
    let hints = std::collections::HashMap::<&str, zbus::zvariant::Value<'_>>::new();
    let _ = proxy
        .notify("linux-flow", 0, "dialog-information", summary, body, &[], hints, -1)
        .await;
}

fn push_toast(toast_tx: &Arc<Mutex<Option<std_mpsc::Sender<String>>>>, msg: String) -> bool {
    if let Ok(guard) = toast_tx.lock() {
        if let Some(tx) = guard.as_ref() {
            return tx.send(msg).is_ok();
        }
    }
    false
}

pub struct Notifier {
    tray: TrayHandle,
    toast_tx: Arc<Mutex<Option<std_mpsc::Sender<String>>>>,
}

impl Notifier {
    pub fn new(tray: TrayHandle) -> Self {
        Self {
            tray,
            toast_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Call from the GTK main thread when a window with a ToastOverlay opens.
    pub fn set_toast_overlay(&self, overlay: &adw::ToastOverlay) {
        let (tx, rx) = std_mpsc::channel::<String>();
        let overlay = overlay.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            loop {
                match rx.try_recv() {
                    Ok(msg) => {
                        overlay.add_toast(adw::Toast::new(&msg));
                    }
                    Err(std_mpsc::TryRecvError::Empty) => break,
                    Err(std_mpsc::TryRecvError::Disconnected) => {
                        return glib::ControlFlow::Break;
                    }
                }
            }
            glib::ControlFlow::Continue
        });
        if let Ok(mut guard) = self.toast_tx.lock() {
            *guard = Some(tx);
        }
    }

    /// Call from the GTK main thread when the window closes.
    pub fn clear_toast(&self) {
        if let Ok(mut guard) = self.toast_tx.lock() {
            *guard = None;
        }
    }

    /// Spawn as a tokio task; consumes self.
    pub async fn run(self, mut rx: mpsc::Receiver<PipelineEvent>) {
        while let Some(event) = rx.recv().await {
            let (state, note): (TrayState, Option<(String, String)>) = match event {
                PipelineEvent::RecordingStarted => (TrayState::Recording, None),
                PipelineEvent::ProcessingStarted => (TrayState::Processing, None),
                PipelineEvent::TextInjected => (TrayState::Idle, None),
                PipelineEvent::Error(msg) => {
                    (TrayState::Error, Some(("linux-flow error".into(), msg)))
                }
                PipelineEvent::LlmFallback => (
                    TrayState::Idle,
                    Some(("linux-flow".into(), "LLM unavailable — used raw transcript".into())),
                ),
            };
            self.tray.set_state(state).await;
            if let Some((summary, body)) = note {
                if !push_toast(&self.toast_tx, body.clone()) {
                    notify_dbus(&summary, &body).await;
                }
            }
        }
    }
}
