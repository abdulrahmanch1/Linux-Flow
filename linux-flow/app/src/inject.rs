use linux_flow_shared::ipc_proto::{encode_frame, IpcRequest, RESPONSE_OK};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};
use zeroize::Zeroize;

const IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct IpcClient {
    stream: Option<UnixStream>,
    path: String,
}

impl IpcClient {
    pub fn new() -> Self {
        // SAFETY: getuid() has no preconditions and always succeeds on Linux.
        let uid = unsafe { libc::getuid() };
        Self {
            stream: None,
            path: format!("/run/user/{uid}/linux-flow-helper.sock"),
        }
    }

    /// Opens the socket if not already connected. Call before the injection
    /// hot path to amortize the connect syscall into idle time (e.g. during ASR).
    pub async fn ensure_connected(&mut self) -> Result<(), String> {
        if self.stream.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Injects `text` via the helper socket. Reconnects once if the stream is
    /// stale (e.g. daemon restarted between sessions).
    ///
    /// `x11_window` is the X11 window ID captured at hotkey press time. `Some`
    /// means the target is XWayland/X11; `None` means Wayland-native.
    pub async fn inject_text(&mut self, text: &str, x11_window: Option<u64>) -> Result<(), String> {
        match self.do_inject(text, x11_window).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.stream = None;
                self.do_inject(text, x11_window).await.map_err(|_| e)
            }
        }
    }

    async fn connect(&mut self) -> Result<(), String> {
        let s = timeout(IPC_TIMEOUT, UnixStream::connect(&self.path))
            .await
            .map_err(|_| "Input helper is not responding. Try restarting linux-flow.".to_string())?
            .map_err(|_| "Input helper is not running. Try restarting linux-flow.".to_string())?;
        self.stream = Some(s);
        Ok(())
    }

    async fn do_inject(&mut self, text: &str, x11_window: Option<u64>) -> Result<(), String> {
        if self.stream.is_none() {
            self.connect().await?;
        }

        let mut ipc_req = IpcRequest::InjectText {
            text: text.to_owned(),
            x11_window,
        };
        let mut frame = encode_frame(&ipc_req).map_err(|e| e.to_string())?;
        let IpcRequest::InjectText { text: ref mut t, .. } = ipc_req;
        t.zeroize();

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| "Input helper is not running. Try restarting linux-flow.".to_string())?;

        timeout(IPC_TIMEOUT, stream.write_all(&frame))
            .await
            .map_err(|_| "Timed out sending to helper.".to_string())?
            .map_err(|_| "Could not inject text into the focused window.".to_string())?;
        frame.zeroize();

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
}
