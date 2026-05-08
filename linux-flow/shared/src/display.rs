#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

pub fn detect() -> DisplayServer {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        DisplayServer::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        DisplayServer::X11
    } else {
        DisplayServer::Unknown
    }
}
