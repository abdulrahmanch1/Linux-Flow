use crate::{error::DaemonError, wayland_clipboard::WaylandClipboard};
use linux_flow_shared::display::detect;
pub use linux_flow_shared::display::DisplayServer;
use zeroize::Zeroize;

/// How long to keep the X11 clipboard selection alive after `set_text()`.
/// Must exceed the browser's clipboard-read round-trip (renderer→browser→X11).
const X11_KEEPALIVE: std::time::Duration = std::time::Duration::from_secs(2);

pub fn detect_display() -> DisplayServer {
    detect()
}

/// Writes `text` to the system clipboard, blocking until the selection is owned
/// by this process. Safe to call Ctrl+V immediately after this returns.
///
/// On X11, a background thread keeps the arboard owner alive for `X11_KEEPALIVE`
/// to serve `SelectionRequest` events after this function returns.
/// Must be called from a blocking thread.
pub fn clipboard_write(
    display: DisplayServer,
    text: &str,
    wl: &WaylandClipboard,
) -> Result<(), DaemonError> {
    match display {
        DisplayServer::Wayland => wl.set(text.to_string()),
        DisplayServer::X11 => {
            let mut owned = text.to_string();
            let mut cb = arboard::Clipboard::new()
                .map_err(|e| DaemonError::InjectionFailed(format!("arboard new: {e}")))?;
            cb.set_text(owned.as_str())
                .map_err(|e| DaemonError::InjectionFailed(format!("arboard set: {e}")))?;
            owned.zeroize();
            // Selection ownership is established; move `cb` into a thread so it
            // stays alive to serve SelectionRequest events until paste completes.
            std::thread::spawn(move || {
                std::thread::sleep(X11_KEEPALIVE);
                drop(cb);
            });
            Ok(())
        }
        DisplayServer::Unknown => Err(DaemonError::InjectionFailed("no display server".into())),
    }
}
