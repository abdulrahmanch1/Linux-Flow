use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::hotkey::{HotkeyError, HotkeyEvent};

const SHORTCUT_ID: &str = "record";

pub struct WaylandBackend;

impl WaylandBackend {
    pub async fn run(tx: mpsc::Sender<HotkeyEvent>) -> Result<(), HotkeyError> {
        let shortcuts = GlobalShortcuts::new().await?;
        let session = shortcuts.create_session().await?;

        shortcuts
            .bind_shortcuts(
                &session,
                &[NewShortcut::new(SHORTCUT_ID, "Push to Talk — linux-flow")
                    .preferred_trigger("Control_R")],
                None,
            )
            .await?
            .response()?;

        let mut activated = shortcuts.receive_activated().await?;
        let mut deactivated = shortcuts.receive_deactivated().await?;

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
            }
        }
    }
}
