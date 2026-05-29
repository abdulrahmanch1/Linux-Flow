use crate::wayland_vk::VirtualKeyboard;
use crate::{error::DaemonError, keys, scancode, uinput_inner::Inner};
use linux_flow_shared::display::DisplayServer;
use std::io::Write;
use std::process::{Command, Stdio};

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
                if let Some(id) = x11_window {
                    // Target is an XWayland window — route through X11 directly.
                    eprintln!("linux-flow-helper: text injector = xdotool (XWayland window {id})");
                    if xdotool_type(text, Some(id)).is_ok() {
                        return Ok(());
                    }
                    // Window may have closed between press and inject; fall through to VK.
                    eprintln!("linux-flow-helper: xdotool failed for window {id}, fallback to vk");
                }
                vk.type_text(text)
            }
            TextInjector::X11 => xdotool_type(text, x11_window),
            TextInjector::Uinput => {
                eprintln!("linux-flow-helper: text injector = uinput (ASCII only, non-ASCII skipped)");
                type_ascii_uinput(inner, text)
            }
        }
    }
}

/// Types `text` via `xdotool type`, reading from stdin so the transcript never
/// appears in the process argument list (`/proc/<pid>/cmdline`).
fn xdotool_type(text: &str, x11_window: Option<u64>) -> Result<(), DaemonError> {
    let mut cmd = Command::new("/usr/bin/xdotool");
    cmd.arg("type").arg("--clearmodifiers");
    if let Some(id) = x11_window {
        cmd.args(["--window", &id.to_string()]);
    }
    cmd.args(["--file", "-"]).stdin(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| DaemonError::InjectionFailed(format!("xdotool spawn: {e}")))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| DaemonError::InjectionFailed("xdotool stdin unavailable".into()))?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| DaemonError::InjectionFailed(format!("xdotool write: {e}")))?;
    }
    let status = child
        .wait()
        .map_err(|e| DaemonError::InjectionFailed(format!("xdotool wait: {e}")))?;
    if !status.success() {
        return Err(DaemonError::InjectionFailed(format!(
            "xdotool exited with {:?}",
            status.code()
        )));
    }
    Ok(())
}

/// Last-resort raw-uinput typing. Only US-layout ASCII is reproducible; any
/// other character is skipped (a warning is logged once).
fn type_ascii_uinput(inner: &mut Inner, text: &str) -> Result<(), DaemonError> {
    let mut skipped = false;
    for c in text.chars() {
        match scancode::ascii_scancode(c) {
            Some((code, shift)) => {
                if shift {
                    inner.key_event(keys::KEY_LEFTSHIFT, true)?;
                }
                inner.key_event(code, true)?;
                inner.key_event(code, false)?;
                if shift {
                    inner.key_event(keys::KEY_LEFTSHIFT, false)?;
                }
            }
            None if !skipped => {
                eprintln!("linux-flow-helper: uinput fallback cannot type non-ASCII; skipping");
                skipped = true;
            }
            None => {}
        }
    }
    Ok(())
}
