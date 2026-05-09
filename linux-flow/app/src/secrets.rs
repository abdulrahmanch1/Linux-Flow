use secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;
use thiserror::Error;
use zeroize::Zeroizing;

const SCHEMA: &str = "ai.linux-flow";
const ATTR_SERVICE: &str = "service";

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("libsecret error: {0}")]
    Service(#[from] secret_service::Error),
    #[error("key not found for service: {0}")]
    NotFound(String),
    #[error("invalid UTF-8 in stored secret")]
    Utf8,
}

pub async fn resolve_key(
    env_var: &str,
    service_name: &str,
) -> Result<Zeroizing<String>, SecretsError> {
    if let Ok(val) = std::env::var(env_var) {
        if !val.is_empty() {
            return Ok(Zeroizing::new(val));
        }
    }
    read_from_keyring(service_name).await
}

pub async fn key_exists(service_name: &str) -> bool {
    read_from_keyring(service_name).await.is_ok()
}

pub async fn store_key(service_name: &str, value: &str) -> Result<(), SecretsError> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;
    if collection.is_locked().await? {
        collection.unlock().await?;
    }
    let attrs = HashMap::from([(ATTR_SERVICE, service_name)]);
    let label = format!("linux-flow {service_name} API key");
    collection
        .create_item(&label, attrs, value.as_bytes(), true, "text/plain")
        .await?;
    Ok(())
}

async fn read_from_keyring(service_name: &str) -> Result<Zeroizing<String>, SecretsError> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;
    if collection.is_locked().await? {
        collection.unlock().await?;
    }
    let attrs = HashMap::from([(ATTR_SERVICE, service_name)]);
    let items = collection.search_items(attrs).await?;
    let item = items.first().ok_or_else(|| SecretsError::NotFound(service_name.into()))?;
    let mut raw = item.get_secret().await?;
    let s = std::str::from_utf8(&raw).map_err(|_| SecretsError::Utf8)?.to_owned();
    zeroize::Zeroize::zeroize(&mut raw);
    Ok(Zeroizing::new(s))
}
