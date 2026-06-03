use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const RESPONSE_OK: u8 = 0x00;
pub const RESPONSE_ERR: u8 = 0x01;

/// Maximum allowed payload size — guards against memory exhaustion.
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024; // 1 MB

#[derive(Serialize, Deserialize)]
pub enum IpcRequest {
    InjectText {
        text: String,
        x11_window: Option<u64>,
    },
}

impl std::fmt::Debug for IpcRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcRequest::InjectText { text, x11_window } => write!(
                f,
                "InjectText {{ text: [REDACTED, {} bytes], x11_window: {:?} }}",
                text.len(),
                x11_window,
            ),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("payload too large: {0} bytes (max 1 MB)")]
    PayloadTooLarge(usize),

    #[error("invalid UTF-8 in frame payload")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Encodes an `IpcRequest` into a framed byte vector.
///
/// Wire format: [4-byte big-endian length][UTF-8 JSON payload]
pub fn encode_frame(req: &IpcRequest) -> Result<Vec<u8>, FrameError> {
    let mut json = serde_json::to_string(req)?;
    let payload = json.as_bytes();
    let len = payload.len();
    if len > MAX_PAYLOAD_BYTES {
        json.zeroize();
        return Err(FrameError::PayloadTooLarge(len));
    }
    let mut buf = Vec::with_capacity(4 + len);
    buf.extend_from_slice(&(len as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    json.zeroize();
    Ok(buf)
}

/// Decodes a raw payload (already length-prefixed bytes stripped) into an `IpcRequest`.
pub fn decode_frame(payload: &[u8]) -> Result<IpcRequest, FrameError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(FrameError::PayloadTooLarge(payload.len()));
    }
    let s = std::str::from_utf8(payload)?;
    Ok(serde_json::from_str(s)?)
}

/// Parses a 4-byte big-endian length prefix into a validated payload length.
///
/// Returns `Err(PayloadTooLarge)` if the declared length exceeds the 1 MB cap.
pub fn parse_length(bytes: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(bytes) as usize;
    if len > MAX_PAYLOAD_BYTES {
        return Err(FrameError::PayloadTooLarge(len));
    }
    Ok(len)
}
