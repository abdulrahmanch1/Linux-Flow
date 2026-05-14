#[path = "hotkey_wayland.rs"]
pub(crate) mod wayland_backend;

#[path = "hotkey_x11.rs"]
pub(crate) mod x11_backend;

use linux_flow_shared::display::{self, DisplayServer};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("portal error: {0}")]
    Portal(#[from] ashpd::Error),
    #[error("event stream closed unexpectedly")]
    StreamClosed,
    #[error("failed to open X11 display")]
    X11Connect,
    #[error("Right Control key not found in X11 keymap")]
    X11Keycode,
    #[error("no display server detected (WAYLAND_DISPLAY and DISPLAY unset)")]
    NoDisplay,
    #[error("portal setup timed out")]
    SetupTimeout,
}

pub struct HotkeyManager {
    rx: mpsc::Receiver<HotkeyEvent>,
}

impl HotkeyManager {
    pub fn start(hotkey: &str) -> Result<Self, HotkeyError> {
        let (tx, rx) = mpsc::channel(4);
        let hotkey = hotkey.to_owned();

        match display::detect() {
            DisplayServer::Wayland => {
                tokio::spawn(async move {
                    if let Err(e) = wayland_backend::WaylandBackend::run(tx.clone()).await {
                        tracing::warn!("wayland portal unavailable ({e}), trying X11 fallback");
                        if std::env::var_os("DISPLAY").is_some() {
                            std::thread::spawn(move || {
                                if let Err(e) = x11_backend::X11Backend::run(tx, &hotkey) {
                                    tracing::error!("x11 fallback exited: {e}");
                                }
                            });
                        }
                    }
                });
            }
            DisplayServer::X11 => {
                std::thread::spawn(move || {
                    if let Err(e) = x11_backend::X11Backend::run(tx, &hotkey) {
                        tracing::error!("x11 backend exited: {e}");
                    }
                });
            }
            DisplayServer::Unknown => return Err(HotkeyError::NoDisplay),
        }

        Ok(Self { rx })
    }

    pub async fn next(&mut self) -> Option<HotkeyEvent> {
        self.rx.recv().await
    }
}
