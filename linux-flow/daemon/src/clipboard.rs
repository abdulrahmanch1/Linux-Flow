use crate::error::DaemonError;
use linux_flow_shared::display::detect;
pub use linux_flow_shared::display::DisplayServer;
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub fn detect_display() -> DisplayServer {
    detect()
}

/// Writes `text` to the system clipboard using the appropriate tool.
pub fn clipboard_write(display: DisplayServer, text: &str) -> Result<(), DaemonError> {
    match display {
        DisplayServer::Wayland => wayland_write(text),
        DisplayServer::X11 => pipe("xclip", &["-selection", "clipboard"], text),
        DisplayServer::Unknown => Err(DaemonError::InjectionFailed("no display server".into())),
    }
}

/// Spawns wl-copy with --paste-once and returns immediately.
///
/// --paste-once keeps wl-copy alive as the clipboard owner until one client
/// requests the data, then it exits. We must NOT block on child.wait() here
/// because wl-copy only exits after the paste (Ctrl+V) is consumed.
fn wayland_write(text: &str) -> Result<(), DaemonError> {
    let mut child = Command::new("wl-copy")
        .arg("--paste-once")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| DaemonError::InjectionFailed(format!("wl-copy: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| DaemonError::InjectionFailed("wl-copy: no stdin".into()))?;
    stdin.write_all(text.as_bytes()).map_err(DaemonError::Io)?;
    drop(stdin);
    // wl-copy runs in the background; clipboard_clear (called after injection)
    // evicts it if no paste arrives within the cleanup window.
    Ok(())
}

/// Clears the system clipboard immediately after injection.
pub fn clipboard_clear(display: DisplayServer) {
    match display {
        DisplayServer::Wayland => {
            let _ = Command::new("wl-copy").arg("--clear").status();
        }
        DisplayServer::X11 => {
            let _ = Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(Stdio::null())
                .status();
        }
        DisplayServer::Unknown => {}
    }
}

fn pipe(cmd: &str, args: &[&str], text: &str) -> Result<(), DaemonError> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| DaemonError::InjectionFailed(format!("{cmd}: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| DaemonError::InjectionFailed(format!("{cmd}: no stdin")))?;
    stdin.write_all(text.as_bytes()).map_err(DaemonError::Io)?;
    drop(stdin);
    let status = child.wait().map_err(DaemonError::Io)?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "killed by signal".into());
        return Err(DaemonError::InjectionFailed(format!(
            "{cmd} exited with code {code}"
        )));
    }
    Ok(())
}
