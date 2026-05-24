#[path = "hotkey_wayland.rs"]
pub(crate) mod wayland_backend;

#[path = "hotkey_x11.rs"]
pub(crate) mod x11_backend;

#[path = "hotkey_evdev.rs"]
pub(crate) mod evdev_backend;

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
    #[error("configured hotkey is not supported on X11")]
    X11Keycode,
    #[error("no display server detected (WAYLAND_DISPLAY and DISPLAY unset)")]
    NoDisplay,
    #[error("portal setup timed out")]
    SetupTimeout,
    #[error("configured hotkey has no evdev device or is not supported")]
    EvdevUnsupported,
}

pub struct HotkeyManager {
    rx: mpsc::Receiver<HotkeyEvent>,
}

impl HotkeyManager {
    pub fn start(hotkey: &str) -> Result<Self, HotkeyError> {
        // raw_tx is shared by all backends; dedup_tx is what the pipeline reads.
        let (raw_tx, raw_rx) = mpsc::channel::<HotkeyEvent>(8);
        let (dedup_tx, dedup_rx) = mpsc::channel::<HotkeyEvent>(4);

        // Deduplicator: suppress consecutive identical events that arise when
        // multiple backends (e.g. X11 + evdev) are running simultaneously.
        tokio::spawn(async move {
            let mut pressed = false;
            let mut rx = raw_rx;
            while let Some(ev) = rx.recv().await {
                let forward = match ev {
                    HotkeyEvent::Pressed if !pressed => {
                        pressed = true;
                        true
                    }
                    HotkeyEvent::Released if pressed => {
                        pressed = false;
                        true
                    }
                    _ => false,
                };
                if forward && dedup_tx.send(ev).await.is_err() {
                    break;
                }
            }
        });

        let hotkey = hotkey.to_owned();
        match display::detect() {
            DisplayServer::Wayland => {
                tokio::spawn(async move {
                    if let Err(e) =
                        wayland_backend::WaylandBackend::run(raw_tx.clone(), hotkey.clone()).await
                    {
                        tracing::warn!("wayland portal unavailable ({e}), trying X11 + evdev");
                        if std::env::var_os("DISPLAY").is_some() {
                            let tx = raw_tx.clone();
                            let hk = hotkey.clone();
                            std::thread::spawn(move || {
                                if let Err(e) = x11_backend::X11Backend::run(tx, &hk) {
                                    tracing::warn!("x11 exited: {e}");
                                }
                            });
                        }
                        if let Err(e) = evdev_backend::EvdevBackend::run(raw_tx, &hotkey).await {
                            tracing::error!("evdev fallback exited: {e}");
                        }
                    }
                });
            }
            DisplayServer::X11 => {
                tokio::spawn(async move {
                    let tx = raw_tx.clone();
                    let hk = hotkey.clone();
                    let result =
                        tokio::task::spawn_blocking(move || x11_backend::X11Backend::run(tx, &hk))
                            .await
                            .unwrap_or_else(|_| Err(HotkeyError::StreamClosed));
                    if let Err(e) = result {
                        tracing::warn!("x11 exited: {e}, trying evdev fallback");
                        if let Err(e) = evdev_backend::EvdevBackend::run(raw_tx, &hotkey).await {
                            tracing::error!("evdev fallback exited: {e}");
                        }
                    }
                });
            }
            DisplayServer::Unknown => return Err(HotkeyError::NoDisplay),
        }

        Ok(Self { rx: dedup_rx })
    }

    pub async fn next(&mut self) -> Option<HotkeyEvent> {
        self.rx.recv().await
    }
}
