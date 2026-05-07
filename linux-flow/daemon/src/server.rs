use std::{
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroize;

use linux_flow_shared::ipc_proto::{self, IpcRequest, RESPONSE_ERR, RESPONSE_OK};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::oneshot,
};

use crate::error::DaemonError;

pub trait KeyInjector: Send + Sync {
    fn inject_text(&self, text: &str) -> Result<(), DaemonError>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the socket path for the current user.
pub fn socket_path() -> Result<PathBuf, DaemonError> {
    // SAFETY: getuid() has no preconditions and always succeeds on Linux.
    let uid = unsafe { libc::getuid() };
    Ok(PathBuf::from(format!(
        "/run/user/{}/linux-flow-helper.sock",
        uid
    )))
}

/// Removes a stale socket file from a previous run, ignoring NotFound.
async fn remove_stale_socket(path: &Path) -> Result<(), DaemonError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(DaemonError::Io(e)),
    }
}

/// Verifies that the connecting peer shares the daemon's UID.
///
/// SAFETY: getsockopt(SO_PEERCRED) is a read-only syscall on a valid fd.
/// The ucred struct is stack-allocated, fully owned, and not aliased.
fn verify_peer(stream: &UnixStream, owner_uid: u32) -> Result<(), DaemonError> {
    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };

    if ret != 0 {
        return Err(DaemonError::Io(std::io::Error::last_os_error()));
    }
    if cred.uid != owner_uid {
        return Err(DaemonError::UcredMismatch {
            expected: owner_uid,
            actual: cred.uid,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

async fn handle_connection(
    mut stream: UnixStream,
    owner_uid: u32,
    injector: Arc<dyn KeyInjector>,
) -> Result<(), DaemonError> {
    verify_peer(&stream, owner_uid)?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let payload_len = ipc_proto::parse_length(len_buf)?;

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload).await?;

    let response = match ipc_proto::decode_frame(&payload) {
        Ok(IpcRequest::InjectText { mut text }) => {
            let result = injector.inject_text(&text);
            text.zeroize();
            match result {
                Ok(()) => RESPONSE_OK,
                Err(_) => RESPONSE_ERR,
            }
        }
        Err(_) => RESPONSE_ERR,
    };
    payload.zeroize();

    stream.write_all(&[response]).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

pub async fn run_server(
    mut shutdown: oneshot::Receiver<()>,
    injector: Arc<dyn KeyInjector>,
) -> Result<(), DaemonError> {
    let path = socket_path()?;
    remove_stale_socket(&path).await?;
    let listener = UnixListener::bind(&path)?;

    // SAFETY: getuid() is always valid on Linux.
    let owner_uid = unsafe { libc::getuid() };

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result?;
                let inj = Arc::clone(&injector);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, owner_uid, inj).await {
                        eprintln!("linux-flow-helper: connection error: {e}");
                    }
                });
            }
            _ = &mut shutdown => {
                // Drop the listener first so the fd is closed before we unlink.
                drop(listener);
                let _ = tokio::fs::remove_file(&path).await;
                break;
            }
        }
    }

    Ok(())
}
