//! Low-level typing backends used by [`crate::paste_trigger::TextInjector`].
//! Each is stateless and reports failure so the injector can fall through to a
//! lower tier without duplicating already-typed text.

use crate::{error::DaemonError, keys, scancode, uinput_inner::Inner};
use std::io::Write;
use std::process::{Command, Stdio};

/// Types `text` via `xdotool type`. When a captured `x11_window` is given but
/// the window has closed between hotkey press and injection, retries against the
/// window that currently holds input focus instead of failing outright.
pub fn xdotool_type(text: &str, x11_window: Option<u64>) -> Result<(), DaemonError> {
    if let Some(id) = x11_window {
        match run_xdotool(text, Some(id)) {
            Ok(()) => return Ok(()),
            Err(e) => {
                // Window likely gone; retry on the current focus. Safe because a
                // targeting failure errors before any character is typed.
                eprintln!("linux-flow-helper: xdotool window {id} failed ({e}); retry on focus");
            }
        }
    }
    run_xdotool(text, None)
}

/// Runs a single `xdotool type`, reading text from stdin so the transcript never
/// appears in the process argument list (`/proc/<pid>/cmdline`).
fn run_xdotool(text: &str, window: Option<u64>) -> Result<(), DaemonError> {
    let mut cmd = Command::new("/usr/bin/xdotool");
    cmd.arg("type").arg("--clearmodifiers");
    if let Some(id) = window {
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
pub fn type_ascii_uinput(inner: &mut Inner, text: &str) -> Result<(), DaemonError> {
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
