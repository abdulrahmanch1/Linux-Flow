use linux_flow_shared::ipc_proto::{encode_frame, IpcRequest, RESPONSE_OK};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};
use zeroize::Zeroize;

const IPC_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) async fn inject_text(text: &str) -> Result<(), String> {
    // SAFETY: getuid() has no preconditions and always succeeds on Linux.
    let uid = unsafe { libc::getuid() };
    let path = format!("/run/user/{uid}/linux-flow-helper.sock");

    let mut stream = timeout(IPC_TIMEOUT, UnixStream::connect(&path))
        .await
        .map_err(|_| "Input helper did not respond. Try restarting linux-flow.".to_string())?
        .map_err(|_| "Input helper is not running. Try restarting linux-flow.".to_string())?;

    let mut frame = encode_frame(&IpcRequest::InjectText { text: text.to_owned() })
        .map_err(|e| e.to_string())?;

    let write_res = timeout(IPC_TIMEOUT, stream.write_all(&frame)).await;
    frame.zeroize();
    write_res
        .map_err(|_| "Could not inject text into the focused window.".to_string())?
        .map_err(|_| "Could not inject text into the focused window.".to_string())?;

    let mut resp = [0u8; 1];
    timeout(IPC_TIMEOUT, stream.read_exact(&mut resp))
        .await
        .map_err(|_| "Could not inject text into the focused window.".to_string())?
        .map_err(|_| "Could not inject text into the focused window.".to_string())?;

    if resp[0] == RESPONSE_OK {
        Ok(())
    } else {
        Err("Could not inject text into the focused window.".to_string())
    }
}
