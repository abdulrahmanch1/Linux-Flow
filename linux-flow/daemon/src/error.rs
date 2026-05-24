use linux_flow_shared::ipc_proto::FrameError;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ipc frame error: {0}")]
    Frame(#[from] FrameError),

    #[error("ucred mismatch: expected uid {expected}, got {actual}")]
    UcredMismatch { expected: u32, actual: u32 },

    #[error("socket path error: {0}")]
    SocketPath(String),

    #[error("injection failed: {0}")]
    InjectionFailed(String),

    #[error("daemon is already running")]
    AlreadyRunning,
}
