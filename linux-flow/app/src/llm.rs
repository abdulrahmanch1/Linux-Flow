use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

const SYSTEM_PROMPT: &str = "You are a transcript editor. \
Apply these transformations and output only the result:\n\
- Remove filler words (um, uh, like, you know, so, basically, literally, \
actually) when used as fillers.\n\
- Remove false starts and self-corrections.\n\
- Remove word repetitions from speech disfluency.\n\
- Fix grammar and sentence flow without adding or changing meaning.\n\
- Apply correct punctuation and capitalization.\n\
- Translate spoken \"new line\" into a literal newline character.\n\
- Translate spoken \"new paragraph\" into two newline characters.\n\
- Preserve the original input language. Never translate.\n\
- Do not add, invent, or infer content not spoken.\n\
- Output only the cleaned text. No preamble, explanation, or quotes.";

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("request timed out (5 s)")]
    Timeout,
    #[error("invalid OpenRouter API key (401)")]
    Unauthorized,
    #[error("empty or malformed response")]
    EmptyResponse,
    #[error("http {0}")]
    Http(u16),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
}

// ── request types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct Provider<'a> {
    sort: &'a str,
}

#[derive(Serialize)]
struct Payload<'a> {
    model: &'a str,
    messages: &'a [ChatMessage<'a>],
    provider: Provider<'a>,
}

// ── response types ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OrMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OrChoice {
    message: OrMessage,
}

#[derive(Deserialize)]
struct OrResponse {
    choices: Vec<OrChoice>,
}

// ── client ───────────────────────────────────────────────────────────────────

/// Reusable HTTP client. Create once at startup; reuses the TCP connection pool.
pub struct LlmClient {
    inner: Client,
}

impl LlmClient {
    pub fn new() -> Result<Self, LlmError> {
        let inner = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(LlmError::Reqwest)?;
        Ok(Self { inner })
    }

    /// Send `transcript` to OpenRouter for LLM editing. Returns the cleaned text.
    ///
    /// On error the caller should fall back to the raw transcript per PRD §10.
    pub async fn rewrite(
        &self,
        mut api_key: Zeroizing<String>,
        model: &str,
        transcript: &str,
    ) -> Result<String, LlmError> {
        let messages = [
            ChatMessage { role: "system", content: SYSTEM_PROMPT },
            ChatMessage { role: "user", content: transcript },
        ];
        let payload = Payload {
            model,
            messages: &messages,
            provider: Provider { sort: "throughput" },
        };

        // Build auth header string, then zeroize api_key — reqwest copies the
        // header value before the future is polled, so the key never lingers.
        let mut auth = Zeroizing::new(format!("Bearer {}", *api_key));
        api_key.zeroize();

        let resp = self
            .inner
            .post(OPENROUTER_URL)
            .header("Authorization", auth.as_str())
            .json(&payload)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { LlmError::Timeout } else { LlmError::Reqwest(e) })?;

        auth.zeroize();

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LlmError::Unauthorized);
        }
        if !status.is_success() {
            return Err(LlmError::Http(status.as_u16()));
        }

        let body: OrResponse = resp
            .json()
            .await
            .map_err(|e| if e.is_timeout() { LlmError::Timeout } else { LlmError::Reqwest(e) })?;

        body.choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .filter(|s| !s.trim().is_empty())
            .ok_or(LlmError::EmptyResponse)
    }
}
