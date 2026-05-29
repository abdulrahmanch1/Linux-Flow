use crate::inject_backends::{type_ascii_uinput, xdotool_type};
use crate::wayland_vk::VirtualKeyboard;
use crate::{error::DaemonError, uinput_inner::Inner};
use linux_flow_shared::display::DisplayServer;

/// Selects how synthesized text reaches the focused window. All paths type the
/// text directly — no clipboard, no paste shortcut.
pub enum TextInjector {
    /// Wayland virtual keyboard with a dynamic per-run XKB keymap.
    /// Layout-independent and Unicode-complete.
    Wayland(VirtualKeyboard),
    /// X11 path: `xdotool type` sends keysyms (layout-independent), reaching
    /// XWayland-hosted apps via input focus.
    X11,
    /// Best-effort ASCII-only fallback for headless / unknown display servers.
    Uinput,
}

impl TextInjector {
    pub fn new(display: DisplayServer) -> Self {
        match display {
            DisplayServer::Wayland => match VirtualKeyboard::new() {
                Some(vk) => {
                    eprintln!("linux-flow-helper: text injector = wayland-vk");
                    TextInjector::Wayland(vk)
                }
                None => {
                    // zwp_virtual_keyboard_v1 unavailable (common on older GNOME).
                    // If XWayland is running ($DISPLAY set), xdotool can reach
                    // XWayland-hosted apps via XGetInputFocus.
                    if std::env::var_os("DISPLAY").is_some() {
                        eprintln!("linux-flow-helper: text injector = xdotool (wayland-vk unavailable, XWayland detected)");
                        TextInjector::X11
                    } else {
                        eprintln!("linux-flow-helper: text injector = uinput (last-resort, ASCII only)");
                        TextInjector::Uinput
                    }
                }
            },
            DisplayServer::X11 => {
                eprintln!("linux-flow-helper: text injector = xdotool");
                TextInjector::X11
            }
            DisplayServer::Unknown => {
                eprintln!("linux-flow-helper: text injector = uinput (display unknown)");
                TextInjector::Uinput
            }
        }
    }

    /// Types `text` into the target window.
    ///
    /// `x11_window` identifies the X11/XWayland window captured at hotkey press.
    /// When `Some(id)`, xdotool targets it explicitly — reliable for XWayland
    /// apps (Chrome, Firefox on GNOME Wayland).
    pub fn fire(
        &mut self,
        inner: &mut Inner,
        text: &str,
        x11_window: Option<u64>,
    ) -> Result<(), DaemonError> {
        match self {
            TextInjector::Wayland(vk) => {
                // For an XWayland target, xdotool reaches it most reliably; otherwise
                // the virtual keyboard is primary. Each tier is attempted only when the
                // one above it errors *before* emitting text, so nothing is duplicated.
                if let Some(id) = x11_window {
                    eprintln!("linux-flow-helper: inject try 1 = xdotool (XWayland window {id})");
                    if let Err(e) = xdotool_type(text, Some(id)) {
                        eprintln!("linux-flow-helper: xdotool failed ({e}); try 2 = wayland-vk");
                        if let Err(e) = vk.type_text(text) {
                            eprintln!("linux-flow-helper: wayland-vk failed ({e}); try 3 = uinput");
                            return type_ascii_uinput(inner, text);
                        }
                    }
                    return Ok(());
                }
                if let Err(e) = vk.type_text(text) {
                    eprintln!("linux-flow-helper: wayland-vk failed ({e}); fallback = uinput");
                    return type_ascii_uinput(inner, text);
                }
                Ok(())
            }
            TextInjector::X11 => {
                if let Err(e) = xdotool_type(text, x11_window) {
                    eprintln!("linux-flow-helper: xdotool failed ({e}); fallback = uinput");
                    return type_ascii_uinput(inner, text);
                }
                Ok(())
            }
            TextInjector::Uinput => {
                eprintln!("linux-flow-helper: text injector = uinput (ASCII only, non-ASCII skipped)");
                type_ascii_uinput(inner, text)
            }
        }
    }
}
