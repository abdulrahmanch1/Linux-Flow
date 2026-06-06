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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: extracts the inner fields of an `InjectText` for assertions,
    /// since `IpcRequest` deliberately does not derive `PartialEq`.
    fn unwrap_inject(req: &IpcRequest) -> (&str, Option<u64>) {
        let IpcRequest::InjectText { text, x11_window } = req;
        (text.as_str(), *x11_window)
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let req = IpcRequest::InjectText {
            text: "héllo wörld 🚀".to_owned(),
            x11_window: Some(42),
        };
        let frame = encode_frame(&req).expect("encode");
        // Strip the 4-byte length prefix before decoding.
        let decoded = decode_frame(&frame[4..]).expect("decode");
        let (text, win) = unwrap_inject(&decoded);
        assert_eq!(text, "héllo wörld 🚀");
        assert_eq!(win, Some(42));
    }

    #[test]
    fn round_trips_with_no_window() {
        let req = IpcRequest::InjectText {
            text: "no window".to_owned(),
            x11_window: None,
        };
        let frame = encode_frame(&req).expect("encode");
        let decoded = decode_frame(&frame[4..]).expect("decode");
        let (text, win) = unwrap_inject(&decoded);
        assert_eq!(text, "no window");
        assert_eq!(win, None);
    }

    #[test]
    fn frame_prefix_is_big_endian_payload_length() {
        let req = IpcRequest::InjectText {
            text: "abc".to_owned(),
            x11_window: None,
        };
        let frame = encode_frame(&req).expect("encode");
        let declared = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(declared, frame.len() - 4);
        // The prefix declares the JSON payload length, which must round-trip
        // back through parse_length.
        assert_eq!(
            parse_length([frame[0], frame[1], frame[2], frame[3]]).unwrap(),
            declared
        );
    }

    #[test]
    fn parse_length_accepts_up_to_the_cap() {
        assert_eq!(parse_length(0u32.to_be_bytes()).unwrap(), 0);
        let cap = MAX_PAYLOAD_BYTES as u32;
        assert_eq!(parse_length(cap.to_be_bytes()).unwrap(), MAX_PAYLOAD_BYTES);
    }

    #[test]
    fn parse_length_rejects_over_the_cap() {
        let over = MAX_PAYLOAD_BYTES as u32 + 1;
        assert!(matches!(
            parse_length(over.to_be_bytes()),
            Err(FrameError::PayloadTooLarge(n)) if n == MAX_PAYLOAD_BYTES + 1
        ));
    }

    #[test]
    fn decode_rejects_invalid_utf8() {
        // 0xFF is never valid UTF-8.
        let err = decode_frame(&[0xFF]).unwrap_err();
        assert!(matches!(err, FrameError::InvalidUtf8(_)));
    }

    #[test]
    fn decode_rejects_oversized_payload() {
        let payload = vec![b'x'; MAX_PAYLOAD_BYTES + 1];
        assert!(matches!(
            decode_frame(&payload),
            Err(FrameError::PayloadTooLarge(_))
        ));
    }

    #[test]
    fn decode_rejects_malformed_json() {
        let err = decode_frame(b"not json").unwrap_err();
        assert!(matches!(err, FrameError::Json(_)));
    }

    #[test]
    fn debug_redacts_injected_text() {
        let req = IpcRequest::InjectText {
            text: "super secret passphrase".to_owned(),
            x11_window: Some(7),
        };
        let rendered = format!("{req:?}");
        assert!(rendered.contains("[REDACTED"));
        assert!(rendered.contains("x11_window: Some(7)"));
        assert!(!rendered.contains("super secret passphrase"));
    }
}
