use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::hotkey::{HotkeyError, HotkeyEvent};

const SHORTCUT_ID: &str = "record";
const SETUP_TIMEOUT: Duration = Duration::from_secs(3);
const STOP_POLL: Duration = Duration::from_millis(100);

pub struct WaylandBackend;

impl WaylandBackend {
    pub async fn run(
        tx: mpsc::Sender<HotkeyEvent>,
        hotkey: String,
        stop: Arc<AtomicBool>,
        bound: Arc<AtomicBool>,
    ) -> Result<(), HotkeyError> {
        let shortcuts = tokio::time::timeout(SETUP_TIMEOUT, GlobalShortcuts::new())
            .await
            .map_err(|_| HotkeyError::SetupTimeout)??;

        let session = tokio::time::timeout(SETUP_TIMEOUT, shortcuts.create_session())
            .await
            .map_err(|_| HotkeyError::SetupTimeout)??;

        shortcuts
            .bind_shortcuts(
                &session,
                &[NewShortcut::new(SHORTCUT_ID, "Push to Talk — linux-flow")
                    .preferred_trigger(&*hotkey)],
                None,
            )
            .await?
            .response()?;

        // The compositor accepted the binding — the key is live.
        bound.store(true, Ordering::Release);

        let mut activated = shortcuts.receive_activated().await?;
        let mut deactivated = shortcuts.receive_deactivated().await?;
        let mut shortcuts_changed = shortcuts.receive_shortcuts_changed().await?;

        loop {
            tokio::select! {
                ev = activated.next() => match ev {
                    Some(a) if a.shortcut_id() == SHORTCUT_ID => {
                        if tx.send(HotkeyEvent::Pressed).await.is_err() {
                            return Err(HotkeyError::StreamClosed);
                        }
                    }
                    Some(_) => {}
                    None => return Err(HotkeyError::StreamClosed),
                },
                ev = deactivated.next() => match ev {
                    Some(d) if d.shortcut_id() == SHORTCUT_ID => {
                        if tx.send(HotkeyEvent::Released).await.is_err() {
                            return Err(HotkeyError::StreamClosed);
                        }
                    }
                    Some(_) => {}
                    None => return Err(HotkeyError::StreamClosed),
                },
                ev = shortcuts_changed.next() => match ev {
                    Some(_) => tracing::debug!("global shortcut bindings changed by compositor"),
                    None => return Err(HotkeyError::StreamClosed),
                },
                // Poll the stop flag so a live hotkey reload can tear this loop
                // down. Returning drops `session`/`shortcuts`, closing the
                // portal session cleanly instead of leaking it.
                _ = tokio::time::sleep(STOP_POLL) => {
                    if stop.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                },
            }
        }
    }
}
