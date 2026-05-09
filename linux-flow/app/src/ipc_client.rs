use linux_flow_shared::ipc_proto::{encode_frame, IpcRequest, RESPONSE_OK};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use zeroize::Zeroize;

pub(crate) async fn inject_text(text: &str) -> Result<(), String> {
    // SAFETY: getuid() has no preconditions and always succeeds on Linux.
    let uid = unsafe { libc::getuid() };
    let path = format!("/run/user/{uid}/linux-flow-helper.sock");
    let mut stream = UnixStream::connect(&path)
        .await
        .map_err(|_| "Input helper is not running. Try restarting linux-flow.".to_string())?;
    let mut frame = encode_frame(&IpcRequest::InjectText { text: text.to_owned() })
        .map_err(|e| e.to_string())?;
    let write_result = stream.write_all(&frame).await;
    frame.zeroize();
    write_result
        .map_err(|_| "Could not inject text into the focused window.".to_string())?;
    let mut resp = [0u8; 1];
    stream
        .read_exact(&mut resp)
        .await
        .map_err(|_| "Could not inject text into the focused window.".to_string())?;
    if resp[0] == RESPONSE_OK {
        Ok(())
    } else {
        Err("Could not inject text into the focused window.".to_string())
    }
}
