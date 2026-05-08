#[path = "hotkey_wayland.rs"]
pub(crate) mod wayland_backend;

#[path = "hotkey_x11.rs"]
pub(crate) mod x11_backend;

use thiserror::Error;

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
}
