use std::time::Duration;

use reqwest::Client;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

const OPENROUTER_KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const SONIOX_MODELS_URL: &str = "https://api.soniox.com/v1/models";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Which provider's key is being checked. Each maps to a lightweight,
/// zero-cost authenticated GET that returns 200 for a good key and 401 for a
/// bad one.
#[derive(Clone, Copy)]
pub enum Provider {
    Soniox,
    OpenRouter,
}

impl Provider {
    fn url(self) -> &'static str {
        match self {
            Provider::Soniox => SONIOX_MODELS_URL,
            Provider::OpenRouter => OPENROUTER_KEY_URL,
        }
    }

    /// Human-readable name for surfacing in the wizard status line.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Soniox => "Soniox",
            Provider::OpenRouter => "OpenRouter",
        }
    }
}

#[derive(Debug, Error)]
pub enum KeyCheckError {
    /// The provider answered and rejected the key (401/403) — definitely bad.
    #[error("the API key was rejected")]
    Invalid,
    /// Could not get a verdict (offline, timeout, 5xx, rate limited). The key
    /// might still be fine, so the caller should warn rather than hard-block.
    #[error("could not verify the key: {0}")]
    Unreachable(String),
}

/// Verifies `key` against `provider` with a single short authenticated GET.
///
/// Only a clear rejection (401/403) is treated as `Invalid`; every transient
/// or ambiguous outcome (network error, timeout, 5xx, 429) becomes
/// `Unreachable` so an offline user is never wrongly told their key is bad.
/// The key is zeroized as soon as the auth header is built.
pub async fn check(provider: Provider, mut key: Zeroizing<String>) -> Result<(), KeyCheckError> {
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| KeyCheckError::Unreachable(e.to_string()))?;

    // Build the header string, then zeroize the key — reqwest copies the header
    // value before the request is sent, so the key never lingers past this line.
    let mut auth = Zeroizing::new(format!("Bearer {}", *key));
    key.zeroize();

    let resp = client
        .get(provider.url())
        .header("Authorization", auth.as_str())
        .send()
        .await;
    auth.zeroize();

    let resp = resp.map_err(|e| KeyCheckError::Unreachable(e.to_string()))?;
    classify(resp.status())
}

/// Maps an HTTP status to a verdict. Only an explicit auth rejection (401/403)
/// proves the key bad; success passes; everything else (5xx, 429, redirects)
/// is inconclusive and must not be reported as an invalid key.
fn classify(status: reqwest::StatusCode) -> Result<(), KeyCheckError> {
    if status.is_success() {
        Ok(())
    } else if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        Err(KeyCheckError::Invalid)
    } else {
        Err(KeyCheckError::Unreachable(format!(
            "HTTP {}",
            status.as_u16()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn success_is_valid() {
        assert!(classify(StatusCode::OK).is_ok());
        assert!(classify(StatusCode::NO_CONTENT).is_ok());
    }

    #[test]
    fn auth_rejection_is_invalid() {
        assert!(matches!(
            classify(StatusCode::UNAUTHORIZED),
            Err(KeyCheckError::Invalid)
        ));
        assert!(matches!(
            classify(StatusCode::FORBIDDEN),
            Err(KeyCheckError::Invalid)
        ));
    }

    #[test]
    fn server_and_transient_errors_are_inconclusive() {
        // A bad gateway, rate limit, or payment-required must never be reported
        // as an invalid key — the key could be perfectly fine.
        for code in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::PAYMENT_REQUIRED,
        ] {
            assert!(
                matches!(classify(code), Err(KeyCheckError::Unreachable(_))),
                "{code} should be inconclusive"
            );
        }
    }
}
