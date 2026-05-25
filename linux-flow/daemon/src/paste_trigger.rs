use std::process::Command;
use crate::{clipboard::DisplayServer, error::DaemonError, keys, uinput_inner::Inner};
use crate::wayland_vk::VirtualKeyboard;

pub enum PasteTrigger {
    /// Wayland virtual keyboard — layout-independent, timestamp-accurate.
    Wayland(VirtualKeyboard),
    /// X11 path: xdotool uses XSendEvent with keysyms, not scancodes — layout-independent.
    X11,
    /// Best-effort fallback for headless / unknown display servers.
    Uinput,
}

impl PasteTrigger {
    pub fn new(display: DisplayServer) -> Self {
        match display {
            DisplayServer::Wayland => {
                match VirtualKeyboard::new() {
                    Some(vk) => {
                        eprintln!("linux-flow-helper: paste trigger = wayland-vk");
                        PasteTrigger::Wayland(vk)
                    }
                    None => {
                        eprintln!("linux-flow-helper: paste trigger = uinput (wayland-vk unavailable)");
                        PasteTrigger::Uinput
                    }
                }
            }
            DisplayServer::X11 => {
                eprintln!("linux-flow-helper: paste trigger = xdotool");
                PasteTrigger::X11
            }
            DisplayServer::Unknown => {
                eprintln!("linux-flow-helper: paste trigger = uinput (display unknown)");
                PasteTrigger::Uinput
            }
        }
    }

    pub fn fire(&mut self, inner: &mut Inner) -> Result<(), DaemonError> {
        match self {
            PasteTrigger::Wayland(vk) => vk.paste(),
            PasteTrigger::X11 => {
                // --clearmodifiers releases any active layout modifier before
                // the event and restores it after, ensuring layout-independence.
                let status = Command::new("/usr/bin/xdotool")
                    .args(["key", "--clearmodifiers", "ctrl+v"])
                    .status()
                    .map_err(|e| DaemonError::InjectionFailed(format!("xdotool: {e}")))?;
                if !status.success() {
                    return Err(DaemonError::InjectionFailed(format!(
                        "xdotool exited with {:?}",
                        status.code()
                    )));
                }
                Ok(())
            }
            PasteTrigger::Uinput => {
                inner.key_event(keys::KEY_PASTE, true)?;
                inner.key_event(keys::KEY_PASTE, false)
            }
        }
    }
}
