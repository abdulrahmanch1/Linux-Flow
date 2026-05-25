use crate::{error::DaemonError, wayland_clipboard::WaylandClipboard};
use linux_flow_shared::display::detect;
pub use linux_flow_shared::display::DisplayServer;
use zeroize::Zeroize;

pub fn detect_display() -> DisplayServer {
    detect()
}

/// Writes `text` to the system clipboard using an in-process owner.
/// Returns immediately — data is served from a background task on demand.
pub fn clipboard_write(
    display: DisplayServer,
    text: &str,
    wl: &WaylandClipboard,
) -> Result<(), DaemonError> {
    match display {
        DisplayServer::Wayland => wl.set(text.to_string()),
        DisplayServer::X11 => {
            let mut owned = text.to_string();
            tokio::task::spawn_blocking(move || {
                let result = (|| {
                    let mut cb = arboard::Clipboard::new().map_err(|e| {
                        DaemonError::InjectionFailed(format!("arboard new: {e}"))
                    })?;
                    cb.set_text(owned.as_str()).map_err(|e| {
                        DaemonError::InjectionFailed(format!("arboard set: {e}"))
                    })?;
                    Ok::<(), DaemonError>(())
                })();
                owned.zeroize();
                // Keep alive so the clipboard selection remains valid until paste completes.
                std::thread::sleep(std::time::Duration::from_secs(2));
                result
            });
            Ok(())
        }
        DisplayServer::Unknown => Err(DaemonError::InjectionFailed("no display server".into())),
    }
}
