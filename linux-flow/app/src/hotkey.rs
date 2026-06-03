#[path = "hotkey_wayland.rs"]
pub(crate) mod wayland_backend;

#[path = "hotkey_x11.rs"]
pub(crate) mod x11_backend;

#[path = "hotkey_evdev.rs"]
pub(crate) mod evdev_backend;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

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
    #[error("XGrabKey failed: key is already grabbed by another process")]
    X11GrabFailed,
}

impl HotkeyError {
    /// Transient errors raised while the XDG GlobalShortcuts portal is still
    /// coming up — worth retrying before falling back to another backend.
    fn is_portal_setup(&self) -> bool {
        matches!(self, HotkeyError::SetupTimeout | HotkeyError::Portal(_))
    }
}

/// Runs the Wayland portal backend, retrying portal setup a few times.
///
/// On a cold boot the portal (e.g. `xdg-desktop-portal-cosmic`) may not be up
/// yet when the app autostarts. Retrying keeps us on the correct Wayland path
/// instead of prematurely committing to the X11/evdev fallback.
async fn run_wayland_with_retry(
    tx: mpsc::Sender<HotkeyEvent>,
    hotkey: String,
    stop: Arc<AtomicBool>,
) -> Result<(), HotkeyError> {
    const MAX_ATTEMPTS: u32 = 5;
    const RETRY_DELAY: Duration = Duration::from_secs(2);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match wayland_backend::WaylandBackend::run(tx.clone(), hotkey.clone(), Arc::clone(&stop))
            .await
        {
            Ok(()) => return Ok(()),
            Err(e) if e.is_portal_setup() && attempt < MAX_ATTEMPTS => {
                tracing::warn!("wayland portal not ready ({e}), retry {attempt}/{MAX_ATTEMPTS}");
                tokio::time::sleep(RETRY_DELAY).await;
            }
            Err(e) => return Err(e),
        }
    }
}

pub struct HotkeyManager {
    rx: mpsc::Receiver<HotkeyEvent>,
    stop: Arc<AtomicBool>,
    parent: Option<tokio::task::JoinHandle<()>>,
    key: String,
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        // Safety net for the final process exit. A live reload uses `shutdown`
        // instead, which also *awaits* the backend's actual teardown.
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl HotkeyManager {
    pub fn start(hotkey: &str) -> Result<Self, HotkeyError> {
        // raw_tx is shared by all backends; dedup_tx is what the pipeline reads.
        let (raw_tx, raw_rx) = mpsc::channel::<HotkeyEvent>(8);
        let (dedup_tx, dedup_rx) = mpsc::channel::<HotkeyEvent>(4);
        let stop = Arc::new(AtomicBool::new(false));

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
        let key = hotkey.clone();
        let parent = match display::detect() {
            DisplayServer::Wayland => {
                let stop = Arc::clone(&stop);
                tokio::spawn(async move {
                    if let Err(e) =
                        run_wayland_with_retry(raw_tx.clone(), hotkey.clone(), Arc::clone(&stop))
                            .await
                    {
                        tracing::warn!("wayland portal unavailable ({e}), trying X11 + evdev");
                        if std::env::var_os("DISPLAY").is_some() {
                            let tx = raw_tx.clone();
                            let hk = hotkey.clone();
                            let s = Arc::clone(&stop);
                            std::thread::spawn(move || {
                                if let Err(e) = x11_backend::X11Backend::run(tx, &hk, s) {
                                    tracing::warn!("x11 exited: {e}");
                                }
                            });
                        }
                        if let Err(e) =
                            evdev_backend::EvdevBackend::run(raw_tx, &hotkey, stop).await
                        {
                            tracing::error!("evdev fallback exited: {e}");
                        }
                    }
                })
            }
            DisplayServer::X11 => {
                let stop = Arc::clone(&stop);
                tokio::spawn(async move {
                    let tx = raw_tx.clone();
                    let hk = hotkey.clone();
                    let s = Arc::clone(&stop);
                    let result = tokio::task::spawn_blocking(move || {
                        x11_backend::X11Backend::run(tx, &hk, s)
                    })
                    .await
                    .unwrap_or_else(|_| Err(HotkeyError::StreamClosed));
                    if let Err(e) = result {
                        tracing::warn!("x11 exited: {e}, trying evdev fallback");
                        if let Err(e) =
                            evdev_backend::EvdevBackend::run(raw_tx, &hotkey, stop).await
                        {
                            tracing::error!("evdev fallback exited: {e}");
                        }
                    }
                })
            }
            DisplayServer::Unknown => return Err(HotkeyError::NoDisplay),
        };

        Ok(Self {
            rx: dedup_rx,
            stop,
            parent: Some(parent),
            key,
        })
    }

    /// The hotkey this manager is currently bound to.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Stops every backend this manager started and *waits* for their teardown
    /// (portal session, X display, evdev devices, inotify thread) before
    /// returning, so a fresh manager can be started without leaking resources
    /// or racing duplicate events. A guard timeout prevents hanging on a wedged
    /// backend.
    pub async fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.parent.take() {
            if tokio::time::timeout(Duration::from_secs(2), handle)
                .await
                .is_err()
            {
                tracing::warn!("hotkey: backend teardown timed out");
            }
        }
    }

    pub async fn next(&mut self) -> Option<HotkeyEvent> {
        self.rx.recv().await
    }
}
