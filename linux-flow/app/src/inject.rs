use linux_flow_shared::ipc_proto::{encode_frame, IpcRequest, RESPONSE_OK};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};
use zeroize::Zeroize;

const IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn inject_text(text: &str) -> Result<(), String> {
    // SAFETY: getuid() has no preconditions and always succeeds on Linux.
    let uid = unsafe { libc::getuid() };
    let path = format!("/run/user/{uid}/linux-flow-helper.sock");
    let mut stream = timeout(IPC_TIMEOUT, UnixStream::connect(&path))
        .await
        .map_err(|_| "Input helper is not responding. Try restarting linux-flow.".to_string())?
        .map_err(|_| "Input helper is not running. Try restarting linux-flow.".to_string())?;

    let mut ipc_req = IpcRequest::InjectText {
        text: text.to_owned(),
    };
    let mut frame = encode_frame(&ipc_req).map_err(|e| e.to_string())?;
    let IpcRequest::InjectText { text: ref mut t } = ipc_req;
    t.zeroize();

    let write_result = timeout(IPC_TIMEOUT, stream.write_all(&frame))
        .await
        .map_err(|_| "Timed out sending to helper.".to_string())?;
    frame.zeroize();
    write_result.map_err(|_| "Could not inject text into the focused window.".to_string())?;

    let mut resp = [0u8; 1];
    timeout(IPC_TIMEOUT, stream.read_exact(&mut resp))
        .await
        .map_err(|_| "Timed out waiting for helper response.".to_string())?
        .map_err(|_| "Could not inject text into the focused window.".to_string())?;

    if resp[0] == RESPONSE_OK {
        Ok(())
    } else {
        Err("Could not inject text into the focused window.".to_string())
    }
}
