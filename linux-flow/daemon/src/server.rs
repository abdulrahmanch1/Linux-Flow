use std::{
    os::unix::{fs::PermissionsExt, io::AsRawFd},
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroize;

use linux_flow_shared::ipc_proto::{self, IpcRequest, RESPONSE_ERR, RESPONSE_OK};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::oneshot,
    time,
};

use crate::error::DaemonError;

pub trait KeyInjector: Send + Sync {
    fn inject_text(&self, text: &str, x11_window: Option<u64>) -> Result<(), DaemonError>;
}

/// Returns the socket path for the current user.
pub fn socket_path() -> Result<PathBuf, DaemonError> {
    // SAFETY: getuid() has no preconditions and always succeeds on Linux.
    let uid = unsafe { libc::getuid() };
    Ok(PathBuf::from(format!(
        "/run/user/{uid}/linux-flow-helper.sock"
    )))
}

/// Ensures the socket path is free for a new bind.
///
/// First attempts to connect to any existing socket. A successful connection
/// means a live daemon is already running — returns `DaemonError::AlreadyRunning`
/// so the caller can exit cleanly without restarting. A failed connection means
/// the file is stale; it is removed so `UnixListener::bind` can succeed.
async fn check_or_remove_socket(path: &Path) -> Result<(), DaemonError> {
    use std::io::ErrorKind;
    match tokio::net::UnixStream::connect(path).await {
        Ok(_) => return Err(DaemonError::AlreadyRunning),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => {} // ECONNREFUSED or similar — stale socket, fall through
    }
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
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

async fn handle_connection(
    mut stream: UnixStream,
    owner_uid: u32,
    injector: Arc<dyn KeyInjector>,
) -> Result<(), DaemonError> {
    verify_peer(&stream, owner_uid)?;

    let mut len_buf = [0u8; 4];
    time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut len_buf),
    )
    .await
    .map_err(|_| {
        DaemonError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "read timeout",
        ))
    })??;
    let payload_len = ipc_proto::parse_length(len_buf)?;

    let mut payload = vec![0u8; payload_len];
    time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut payload),
    )
    .await
    .map_err(|_| {
        DaemonError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "read timeout",
        ))
    })??;

    let response = match ipc_proto::decode_frame(&payload) {
        Ok(IpcRequest::InjectText { mut text, x11_window }) => {
            let inj = Arc::clone(&injector);
            let result = tokio::task::spawn_blocking(move || {
                let r = inj.inject_text(&text, x11_window);
                text.zeroize();
                r
            })
            .await
            .map_err(|e| DaemonError::InjectionFailed(format!("blocking task: {e}")))?;
            match result {
                Ok(()) => RESPONSE_OK,
                Err(e) => {
                    eprintln!("linux-flow-helper: injection failed: {e}");
                    RESPONSE_ERR
                }
            }
        }
        Err(e) => {
            eprintln!("linux-flow-helper: ipc decode error: {e}");
            RESPONSE_ERR
        }
    };
    payload.zeroize();

    stream.write_all(&[response]).await?;
    Ok(())
}

pub async fn run_server(
    mut shutdown: oneshot::Receiver<()>,
    injector: Arc<dyn KeyInjector>,
) -> Result<(), DaemonError> {
    let path = socket_path()?;
    check_or_remove_socket(&path).await?;
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(DaemonError::Io)?;

    // SAFETY: getuid() is always valid on Linux.
    let owner_uid = unsafe { libc::getuid() };

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = match result {
                    Ok(s) => s,
                    Err(e) => { eprintln!("linux-flow-helper: accept error: {e}"); continue; }
                };
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
