use crate::error::DaemonError;
use wl_clipboard_rs::copy::{MimeType, Options, Source};
use zeroize::Zeroize;

pub struct WaylandClipboard;

impl WaylandClipboard {
    pub fn new() -> Self {
        WaylandClipboard
    }

    /// Sets clipboard content in-process. Returns immediately (~0ms) — the
    /// daemon serves paste requests from a background blocking task.
    pub fn set(&self, text: String) -> Result<(), DaemonError> {
        tokio::task::spawn_blocking(move || {
            let mut bytes = text.into_bytes();
            let to_serve = bytes.clone();
            let result = Options::new()
                .copy(Source::Bytes(to_serve.into()), MimeType::Autodetect);
            bytes.zeroize();
            result.map_err(|e| DaemonError::InjectionFailed(format!("wl-clipboard: {e}")))
        });
        Ok(())
    }
}
