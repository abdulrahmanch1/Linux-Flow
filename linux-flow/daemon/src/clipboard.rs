use crate::{error::DaemonError, keys, uinput_inner::Inner};
use linux_flow_shared::display::DisplayServer;
use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

/// Keep the selection owner alive for repeated reads (including XWayland).
/// Never restore the old selection on a timer: a busy client may read it later.
pub fn paste(inner: &mut Inner, text: &str, display: DisplayServer) -> Result<(), DaemonError> {
    if text.is_empty() {
        return Ok(());
    }
    let shift = match std::env::var("LINUX_FLOW_PASTE_SHORTCUT").as_deref() {
        Ok("ctrl-shift-v") => true,
        Ok("ctrl-v") | Err(std::env::VarError::NotPresent) => false,
        _ => {
            return Err(DaemonError::InjectionFailed(
                "LINUX_FLOW_PASTE_SHORTCUT must be ctrl-v or ctrl-shift-v".into(),
            ));
        }
    };
    match display {
        DisplayServer::Wayland => crate::wayland_clipboard::copy(text)?,
        DisplayServer::X11 => copy_x11(text)?,
        DisplayServer::Unknown => {
            return Err(DaemonError::InjectionFailed(
                "clipboard requires a Wayland or X11 session".into(),
            ));
        }
    }
    paste_shortcut(inner, shift)
}

fn copy_x11(text: &str) -> Result<(), DaemonError> {
    // Bound clipboard setup below the application's five-second IPC deadline.
    // timeout also kills the process group if xclip gets stuck.
    let mut command = Command::new("/usr/bin/timeout");
    command.args([
        "--kill-after=1s",
        "3s",
        "/usr/bin/xclip",
        "-selection",
        "clipboard",
        "-in",
        "-target",
        "UTF8_STRING",
    ]);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| DaemonError::InjectionFailed(format!("clipboard spawn: {e}")))?;
    let written = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(text.as_bytes()),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "clipboard stdin unavailable",
        )),
    };
    // Closing stdin above gives xclip EOF. It forks after acquiring the selection;
    // waiting for its parent is the readiness fence.
    let status = child.wait()?;
    written?;
    if !status.success() {
        return Err(DaemonError::InjectionFailed(format!(
            "clipboard setup exited with {status}"
        )));
    }
    Ok(())
}

fn paste_shortcut(inner: &mut Inner, shift: bool) -> Result<(), DaemonError> {
    // Use the actual keyboard layout, not the text backend's remapped keycodes.
    // Always release every modifier, even if an intermediate write fails.
    let result = (|| {
        inner.key_event(keys::KEY_LEFTCTRL, true)?;
        if shift {
            inner.key_event(keys::KEY_LEFTSHIFT, true)?;
        }
        thread::sleep(Duration::from_millis(20));
        inner.key_event(keys::KEY_V, true)?;
        thread::sleep(Duration::from_millis(20));
        Ok(())
    })();
    let release_v = inner.key_event(keys::KEY_V, false);
    let release_shift = if shift {
        inner.key_event(keys::KEY_LEFTSHIFT, false)
    } else {
        Ok(())
    };
    let release_ctrl = inner.key_event(keys::KEY_LEFTCTRL, false);
    result.and(release_v).and(release_shift).and(release_ctrl)
}
