use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

pub fn detect() -> DisplayServer {
    // Fast path: env var already set by the session manager or by a prior probe.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return DisplayServer::Wayland;
    }
    // WAYLAND_DISPLAY absent — probe the well-known socket under XDG_RUNTIME_DIR.
    // pam_systemd sets XDG_RUNTIME_DIR at login, before any graphical session,
    // so it is reliable even when PassEnvironment= hasn't propagated WAYLAND_DISPLAY yet.
    //
    // The probe must be retried on every call until it first succeeds: the daemon
    // and the app can start at boot before the compositor's socket exists (notably
    // on COSMIC, where the graphical session comes up after the user services).
    // A cached negative result would pin the process to the wrong backend for the
    // whole session. A Mutex serialises the set_var so it still happens from a
    // single writer; once it succeeds, the fast path above returns first and this
    // block is never reached again — so set_var effectively runs at most once.
    static PROBE_LOCK: Mutex<()> = Mutex::new(());
    if let Ok(_guard) = PROBE_LOCK.lock() {
        // Re-check inside the lock: another thread may have set it meanwhile.
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            if let Some(name) = probe_wayland_socket() {
                // SAFETY: held under PROBE_LOCK — no concurrent writers.
                unsafe { std::env::set_var("WAYLAND_DISPLAY", &name) };
            }
        }
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return DisplayServer::Wayland;
    }
    if std::env::var_os("DISPLAY").is_some() {
        return DisplayServer::X11;
    }
    DisplayServer::Unknown
}

/// Blocks until a display server is detected or `timeout` elapses, polling at a
/// short interval. Returns the last `detect()` result either way.
///
/// On autostart the app can launch before the compositor's Wayland socket
/// exists; waiting here lets backend selection see the real display instead of
/// committing to `Unknown` (which would otherwise kill the app or pick evdev).
pub fn wait_for_display(timeout: Duration) -> DisplayServer {
    let deadline = Instant::now() + timeout;
    loop {
        let server = detect();
        if server != DisplayServer::Unknown || Instant::now() >= deadline {
            return server;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn probe_wayland_socket() -> Option<std::ffi::OsString> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let base = std::path::Path::new(&runtime_dir);
    // Try wayland-0 first; fall back to wayland-1 for multi-seat setups.
    // Connect (not just stat) to confirm the compositor is actually listening.
    for name in ["wayland-0", "wayland-1"] {
        if std::os::unix::net::UnixStream::connect(base.join(name)).is_ok() {
            return Some(name.into());
        }
    }
    None
}
