use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ModelsError {
    #[error("model list request failed: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// Fetches model ids from OpenRouter's public catalog. No API key is required,
/// and the result is held in memory only (never written to disk).
pub async fn fetch_model_ids() -> Result<Vec<String>, ModelsError> {
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let resp: ModelsResponse = client
        .get(MODELS_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.data.into_iter().map(|m| m.id).collect())
}
