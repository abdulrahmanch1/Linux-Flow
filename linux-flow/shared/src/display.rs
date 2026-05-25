#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

pub fn detect() -> DisplayServer {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return DisplayServer::Wayland;
    }
    // WAYLAND_DISPLAY absent — probe the well-known socket under XDG_RUNTIME_DIR.
    // pam_systemd sets XDG_RUNTIME_DIR at login, before any graphical session,
    // so it is reliable even when PassEnvironment= hasn't propagated WAYLAND_DISPLAY yet.
    if let Some(name) = probe_wayland_socket() {
        // SAFETY: single-threaded init path; called once before any injection thread starts.
        #[allow(unused_unsafe)]
        unsafe { std::env::set_var("WAYLAND_DISPLAY", &name) };
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
    for name in ["wayland-0", "wayland-1"] {
        if base.join(name).exists() {
            return Some(name.into());
        }
    }
    None
}
