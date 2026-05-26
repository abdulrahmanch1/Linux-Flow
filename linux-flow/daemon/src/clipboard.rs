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

/// Writes `text` to the X11 CLIPBOARD selection synchronously.
/// Moves the arboard owner into a keep-alive thread so SelectionRequest events
/// are served until paste completes. Must be called from a blocking thread.
fn write_x11_clipboard(text: &str) -> Result<(), DaemonError> {
    let mut owned = text.to_string();
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| DaemonError::InjectionFailed(format!("arboard new: {e}")))?;
    cb.set_text(owned.as_str())
        .map_err(|e| DaemonError::InjectionFailed(format!("arboard set: {e}")))?;
    owned.zeroize();
    std::thread::spawn(move || {
        std::thread::sleep(X11_KEEPALIVE);
        drop(cb);
    });
    Ok(())
}

/// Writes `text` to the system clipboard, blocking until owned by this process.
///
/// On Wayland sessions with an active XWayland display (`$DISPLAY` set), both
/// the Wayland clipboard and the X11 CLIPBOARD selection are written. This
/// eliminates the XWayland clipboard-sync race: XWayland browsers read the X11
/// selection directly instead of waiting for the compositor-side bridge to sync.
/// Must be called from a blocking thread.
pub fn clipboard_write(
    display: DisplayServer,
    text: &str,
    wl: &WaylandClipboard,
) -> Result<(), DaemonError> {
    match display {
        DisplayServer::Wayland => {
            wl.set(text.to_string())?;
            // Dual-write: if XWayland is active, also write the X11 clipboard so
            // XWayland-hosted browsers (Chrome, Firefox) don't hit a sync race.
            if std::env::var_os("DISPLAY").is_some() {
                write_x11_clipboard(text)?;
            }
            Ok(())
        }
        DisplayServer::X11 => write_x11_clipboard(text),
        DisplayServer::Unknown => Err(DaemonError::InjectionFailed("no display server".into())),
    }
}
