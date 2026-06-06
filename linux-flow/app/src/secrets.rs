use secret_service::{Collection, EncryptionType, SecretService};
use std::collections::HashMap;
use thiserror::Error;
use zeroize::Zeroizing;

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

/// Reads the env override for `env_var`, returning the value only when it is
/// set and non-empty. The keyring is the fallback for every other case.
fn env_override(env_var: &str) -> Option<Zeroizing<String>> {
    match std::env::var(env_var) {
        Ok(val) if !val.is_empty() => Some(Zeroizing::new(val)),
        _ => None,
    }
}

pub async fn resolve_key(
    env_var: &str,
    service_name: &str,
) -> Result<Zeroizing<String>, SecretsError> {
    if let Some(val) = env_override(env_var) {
        return Ok(val);
    }
    read_from_keyring(service_name).await
}

/// Resolves several keys through a **single** Secret Service session, so cold
/// start pays one D-Bus connect + unlock instead of one per key. Each spec is
/// `(env_var, service_name)` and still honors its env override first; the
/// keyring is only contacted when at least one key isn't supplied via the
/// environment. Results are returned in the same order as `specs`.
pub async fn resolve_keys(specs: &[(&str, &str)]) -> Result<Vec<Zeroizing<String>>, SecretsError> {
    // Env-first pass: a resolved slot holds the value, an unresolved one
    // remembers which service still needs the keyring.
    let mut slots: Vec<Result<Zeroizing<String>, &str>> = specs
        .iter()
        .map(|(env_var, service)| env_override(env_var).ok_or(*service))
        .collect();

    if slots.iter().any(|s| s.is_err()) {
        // Open the shared session once for every key that fell through to the
        // keyring, reusing the same connection, collection, and unlock.
        let ss = SecretService::connect(EncryptionType::Dh).await?;
        let collection = ss.get_default_collection().await?;
        if collection.is_locked().await? {
            collection.unlock().await?;
        }
        for slot in slots.iter_mut() {
            if let Err(service) = *slot {
                *slot = Ok(read_item(&collection, service).await?);
            }
        }
    }

    // Every slot is resolved by construction; collect surfaces any stray
    // unresolved service as a NotFound rather than panicking.
    slots
        .into_iter()
        .map(|s| s.map_err(|service| SecretsError::NotFound(service.into())))
        .collect()
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
    read_item(&collection, service_name).await
}

/// Fetches and decodes one secret from an already-connected, unlocked
/// collection. Shared by the single- and multi-key resolvers so the search and
/// zeroizing decode live in exactly one place.
async fn read_item(
    collection: &Collection<'_>,
    service_name: &str,
) -> Result<Zeroizing<String>, SecretsError> {
    let attrs = HashMap::from([(ATTR_SERVICE, service_name)]);
    let items = collection.search_items(attrs).await?;
    let item = items
        .first()
        .ok_or_else(|| SecretsError::NotFound(service_name.into()))?;
    let mut raw = item.get_secret().await?;
    let s = std::str::from_utf8(&raw)
        .map_err(|_| SecretsError::Utf8)?
        .to_owned();
    zeroize::Zeroize::zeroize(&mut raw);
    Ok(Zeroizing::new(s))
}
