use crate::error::DaemonError;
use wl_clipboard_rs::copy::{MimeType, Options, Source};

pub struct WaylandClipboard;

impl WaylandClipboard {
    pub fn new() -> Self {
        WaylandClipboard
    }

    /// Sets the Wayland clipboard selection, blocking until the compositor
    /// acknowledges ownership. Must be called from a blocking thread.
    pub fn set(&self, text: String) -> Result<(), DaemonError> {
        // Move bytes directly — no parent copy remains after the move.
        // wl_clipboard_rs forks a child process; the child's in-memory copy is
        // OS-isolated and freed when the child exits. Cloning would leave an
        // unzeroizable extra copy in the parent's heap.
        let bytes = text.into_bytes();
        Options::new()
            .copy(Source::Bytes(bytes.into()), MimeType::Autodetect)
            .map_err(|e| DaemonError::InjectionFailed(format!("wl-clipboard: {e}")))
    }
}
