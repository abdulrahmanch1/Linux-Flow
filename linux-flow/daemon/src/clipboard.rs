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
        DisplayServer::Wayland => pipe("/usr/bin/wl-copy", &[], text),
        DisplayServer::X11 => pipe("/usr/bin/xclip", &["-selection", "clipboard"], text),
        DisplayServer::Unknown => Err(DaemonError::InjectionFailed("no display server".into())),
    }
}

/// Clears the system clipboard immediately after injection.
pub fn clipboard_clear(display: DisplayServer) {
    match display {
        DisplayServer::Wayland => {
            let _ = Command::new("/usr/bin/wl-copy").arg("--clear").status();
        }
        DisplayServer::X11 => {
            let _ = Command::new("/usr/bin/xclip")
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
    child.wait().map_err(DaemonError::Io)?;
    Ok(())
}
