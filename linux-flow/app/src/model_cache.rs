use std::{
    fs,
    path::{Path, PathBuf},
};

// On-disk cache of OpenRouter's public model catalog (model ids only — public,
// non-sensitive data, so persisting it does not touch the audio/transcript
// zero-persistence rule). It lives under XDG_CACHE_HOME so it carries true
// "cache" semantics: deleting it loses no user setting, only one network
// round-trip. This is what lets model suggestions work offline across restarts
// instead of only within a single process run.

/// `$XDG_CACHE_HOME/linux-flow/models.json`, falling back to
/// `~/.cache/linux-flow/models.json` when the variable is unset or relative.
fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("linux-flow").join("models.json"))
}

/// Reads the last persisted catalog, or an empty vec when it is absent or
/// unreadable. Never errors: a missing or corrupt cache just means "nothing to
/// suggest yet", and the caller falls back to a live fetch.
pub fn load() -> Vec<String> {
    cache_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Persists the catalog, best-effort. Any failure is logged and swallowed: the
/// in-session suggestions still work even if the disk write cannot happen.
pub fn store(ids: &[String]) {
    if let Some(path) = cache_path() {
        store_to(&path, ids);
    }
}

fn load_from(path: &Path) -> Vec<String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn store_to(path: &Path, ids: &[String]) {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            tracing::warn!("model cache: create dir failed: {e}");
            return;
        }
    }
    match serde_json::to_vec(ids) {
        Ok(bytes) => {
            if let Err(e) = fs::write(path, bytes) {
                tracing::warn!("model cache: write failed: {e}");
            }
        }
        Err(e) => tracing::warn!("model cache: serialize failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("linux-flow-test-{}-{tag}", std::process::id()));
        p.push("models.json");
        p
    }

    #[test]
    fn store_then_load_round_trips() {
        let path = temp_file("roundtrip");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let ids = vec!["a/one".to_owned(), "b/two".to_owned()];
        store_to(&path, &ids);
        assert_eq!(load_from(&path), ids);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_loads_empty() {
        let path = temp_file("missing");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(load_from(&path).is_empty());
    }

    #[test]
    fn corrupt_file_loads_empty() {
        let path = temp_file("corrupt");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json {{{").unwrap();
        assert!(load_from(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
