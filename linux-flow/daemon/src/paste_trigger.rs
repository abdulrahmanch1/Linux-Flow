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
                        // zwp_virtual_keyboard_v1 unavailable (common on GNOME).
                        // If XWayland is running ($DISPLAY set), xdotool can reach
                        // XWayland-hosted apps via XGetInputFocus — use that instead
                        // of uinput which browsers universally ignore.
                        if std::env::var_os("DISPLAY").is_some() {
                            eprintln!("linux-flow-helper: paste trigger = xdotool (wayland-vk unavailable, XWayland detected)");
                            PasteTrigger::X11
                        } else {
                            eprintln!("linux-flow-helper: paste trigger = uinput (last-resort, no XWayland)");
                            PasteTrigger::Uinput
                        }
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

    /// Fires the paste trigger for the given target window.
    ///
    /// `x11_window` identifies the X11/XWayland window captured at hotkey press.
    /// When `Some(id)`, xdotool targets that window explicitly — reliable for
    /// XWayland-hosted apps (Chrome, Firefox on GNOME Wayland). When `None`,
    /// the target is Wayland-native and the virtual keyboard or uinput is used.
    pub fn fire(&mut self, inner: &mut Inner, x11_window: Option<u64>) -> Result<(), DaemonError> {
        match self {
            PasteTrigger::Wayland(vk) => {
                if let Some(id) = x11_window {
                    // Target is an XWayland window — route through X11 directly.
                    eprintln!("linux-flow-helper: paste trigger = xdotool (XWayland window {id})");
                    let status = Command::new("/usr/bin/xdotool")
                        .args(["key", "--window", &id.to_string(), "--clearmodifiers", "ctrl+v"])
                        .status()
                        .map_err(|e| DaemonError::InjectionFailed(format!("xdotool: {e}")))?;
                    if status.success() {
                        return Ok(());
                    }
                    // Window may have closed between press and inject; fall through to VK.
                    eprintln!("linux-flow-helper: xdotool failed for window {id}, fallback to vk");
                }
                eprintln!("linux-flow-helper: paste trigger = wayland-vk");
                vk.paste()
            }
            PasteTrigger::X11 => {
                // Use captured window ID when available for accuracy; otherwise
                // let xdotool send to whatever window holds X11 input focus.
                let mut cmd = Command::new("/usr/bin/xdotool");
                if let Some(id) = x11_window {
                    cmd.args(["key", "--window", &id.to_string(), "--clearmodifiers", "ctrl+v"]);
                } else {
                    cmd.args(["key", "--clearmodifiers", "ctrl+v"]);
                }
                let status = cmd
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
                eprintln!("linux-flow-helper: paste trigger = uinput (last-resort, browsers may ignore)");
                inner.key_event(keys::KEY_PASTE, true)?;
                inner.key_event(keys::KEY_PASTE, false)
            }
        }
    }
}
