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
    // OnceLock guarantees the set_var call happens from a single thread even when
    // detect() is called concurrently from multiple spawn_blocking tasks.
    static SOCKET_PROBED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    SOCKET_PROBED.get_or_init(|| {
        if let Some(name) = probe_wayland_socket() {
            // SAFETY: runs exactly once, guaranteed by OnceLock — no concurrent writers.
            unsafe { std::env::set_var("WAYLAND_DISPLAY", &name) };
        }
    });
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return DisplayServer::Wayland;
    }
    if std::env::var_os("DISPLAY").is_some() {
        return DisplayServer::X11;
    }
    DisplayServer::Unknown
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
