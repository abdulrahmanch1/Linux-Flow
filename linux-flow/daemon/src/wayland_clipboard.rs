use crate::error::DaemonError;
use wl_clipboard_rs::copy::{MimeType, Options, Source};
use zeroize::Zeroize;

pub struct WaylandClipboard;

impl WaylandClipboard {
    pub fn new() -> Self {
        WaylandClipboard
    }

    /// Sets the Wayland clipboard selection, blocking until the compositor
    /// acknowledges ownership. Must be called from a blocking thread.
    pub fn set(&self, text: String) -> Result<(), DaemonError> {
        let mut bytes = text.into_bytes();
        let payload = bytes.clone();
        let result = Options::new()
            .copy(Source::Bytes(payload.into()), MimeType::Autodetect)
            .map_err(|e| DaemonError::InjectionFailed(format!("wl-clipboard: {e}")));
        bytes.zeroize();
        result
    }
}
