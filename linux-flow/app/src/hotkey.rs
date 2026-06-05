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
    ///
    /// `PortalNotFound` is deliberately excluded: it means the portal service is
    /// reachable but exposes no GlobalShortcuts frontend (e.g. COSMIC), which
    /// never changes. Retrying it would burn the whole retry budget (~10 s) with
    /// the hotkey unbound — every press in that window is silently lost — before
    /// finally reaching the X11/evdev fallback. Treating it as non-transient
    /// makes that fallback kick in immediately.
    fn is_portal_setup(&self) -> bool {
        match self {
            HotkeyError::SetupTimeout => true,
            HotkeyError::Portal(ashpd::Error::PortalNotFound(_)) => false,
            HotkeyError::Portal(_) => true,
            _ => false,
        }
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
    bound: Arc<AtomicBool>,
) -> Result<(), HotkeyError> {
    const MAX_ATTEMPTS: u32 = 5;
    const RETRY_DELAY: Duration = Duration::from_secs(2);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match wayland_backend::WaylandBackend::run(
            tx.clone(),
            hotkey.clone(),
            Arc::clone(&stop),
            Arc::clone(&bound),
        )
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

/// Waits a short grace period for any parallel backend (e.g. the X11 thread) to
/// raise `bound`, then—if nothing bound and we are not tearing down—reports a
/// user-facing failure so the silent "hotkey does nothing" case becomes visible.
/// Stays quiet during shutdown and whenever a backend did bind, avoiding false
/// alarms when X11 succeeds while evdev is unavailable.
async fn notify_if_unbound(
    bound: &AtomicBool,
    stop: &AtomicBool,
    last_err: Option<HotkeyError>,
    on_fail: &Option<mpsc::Sender<String>>,
) {
    const GRACE: Duration = Duration::from_millis(500);
    if stop.load(Ordering::Relaxed) {
        return;
    }
    tokio::time::sleep(GRACE).await;
    if stop.load(Ordering::Relaxed) || bound.load(Ordering::Acquire) {
        return;
    }
    if let Some(tx) = on_fail {
        let _ = tx.send(failure_message(last_err)).await;
    }
}

/// Builds the user-facing message for a fully failed hotkey chain. The common
/// case—evdev rejected because the user isn't in the `input` group—gets an
/// actionable hint; anything else falls back to the technical error.
fn failure_message(last_err: Option<HotkeyError>) -> String {
    match last_err {
        Some(HotkeyError::EvdevUnsupported) => {
            "Hotkey is not active. linux-flow could not register \
             the push-to-talk key. Add your user to the 'input' group, then log out and back in."
                .to_owned()
        }
        Some(e) => format!("Hotkey is not active: {e}. The push-to-talk key did not register."),
        None => "Hotkey is not active. The push-to-talk key did not register.".to_owned(),
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
    pub fn start(hotkey: &str, on_fail: Option<mpsc::Sender<String>>) -> Result<Self, HotkeyError> {
        // raw_tx is shared by all backends; dedup_tx is what the pipeline reads.
        let (raw_tx, raw_rx) = mpsc::channel::<HotkeyEvent>(8);
        let (dedup_tx, dedup_rx) = mpsc::channel::<HotkeyEvent>(4);
        let stop = Arc::new(AtomicBool::new(false));
        // Raised by whichever backend successfully binds/grabs the key, so the
        // fallback chain can tell "nothing bound" (notify the user) apart from "a
        // parallel backend bound it" (stay quiet).
        let bound = Arc::new(AtomicBool::new(false));

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
                let bound = Arc::clone(&bound);
                tokio::spawn(async move {
                    if let Err(e) = run_wayland_with_retry(
                        raw_tx.clone(),
                        hotkey.clone(),
                        Arc::clone(&stop),
                        Arc::clone(&bound),
                    )
                    .await
                    {
                        tracing::warn!("wayland portal unavailable ({e}), trying X11 + evdev");
                        if std::env::var_os("DISPLAY").is_some() {
                            let tx = raw_tx.clone();
                            let hk = hotkey.clone();
                            let s = Arc::clone(&stop);
                            let b = Arc::clone(&bound);
                            std::thread::spawn(move || {
                                if let Err(e) = x11_backend::X11Backend::run(tx, &hk, s, b) {
                                    tracing::warn!("x11 exited: {e}");
                                }
                            });
                        }
                        let stop_n = Arc::clone(&stop);
                        let evdev = evdev_backend::EvdevBackend::run(
                            raw_tx,
                            &hotkey,
                            stop,
                            Arc::clone(&bound),
                        )
                        .await;
                        if let Err(e) = &evdev {
                            tracing::error!("evdev fallback exited: {e}");
                        }
                        notify_if_unbound(&bound, &stop_n, evdev.err(), &on_fail).await;
                    }
                })
            }
            DisplayServer::X11 => {
                let stop = Arc::clone(&stop);
                let bound = Arc::clone(&bound);
                tokio::spawn(async move {
                    let tx = raw_tx.clone();
                    let hk = hotkey.clone();
                    let s = Arc::clone(&stop);
                    let b = Arc::clone(&bound);
                    let result = tokio::task::spawn_blocking(move || {
                        x11_backend::X11Backend::run(tx, &hk, s, b)
                    })
                    .await
                    .unwrap_or_else(|_| Err(HotkeyError::StreamClosed));
                    if let Err(e) = result {
                        tracing::warn!("x11 exited: {e}, trying evdev fallback");
                        let stop_n = Arc::clone(&stop);
                        let evdev = evdev_backend::EvdevBackend::run(
                            raw_tx,
                            &hotkey,
                            stop,
                            Arc::clone(&bound),
                        )
                        .await;
                        if let Err(e) = &evdev {
                            tracing::error!("evdev fallback exited: {e}");
                        }
                        notify_if_unbound(&bound, &stop_n, evdev.err(), &on_fail).await;
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
