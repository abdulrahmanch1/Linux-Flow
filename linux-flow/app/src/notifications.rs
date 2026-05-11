use crate::{
    pipeline::PipelineEvent,
    tray::{TrayHandle, TrayState},
};
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

pub struct Notifier {
    tray: TrayHandle,
}

impl Notifier {
    pub fn new(tray: TrayHandle) -> Self {
        Self { tray }
    }

    pub async fn run(self, mut rx: mpsc::Receiver<PipelineEvent>) {
        while let Some(event) = rx.recv().await {
            let (state, note): (TrayState, Option<(&'static str, String)>) = match event {
                PipelineEvent::RecordingStarted => (TrayState::Recording, None),
                PipelineEvent::ProcessingStarted => (TrayState::Processing, None),
                PipelineEvent::TextInjected => (TrayState::Idle, None),
                PipelineEvent::Error(msg) => {
                    (TrayState::Error, Some(("linux-flow error", msg)))
                }
                PipelineEvent::LlmFallback => (
                    TrayState::Idle,
                    Some(("linux-flow", "LLM unavailable — used raw transcript".into())),
                ),
            };
            self.tray.set_state(state).await;
            if let Some((summary, body)) = note {
                notify_dbus(summary, &body).await;
            }
        }
    }
}
