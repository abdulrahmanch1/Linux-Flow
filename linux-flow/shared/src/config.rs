use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("no home directory")]
    NoHome,
}

// Container-level `#[serde(default)]` on every struct keeps an older
// `config.toml` loadable after an upgrade adds a field: a missing field — or a
// whole missing table — falls back to that struct's `Default` instead of
// failing the parse. Each struct owns its defaults so there is a single source
// of truth (field-level `#[serde(default)]` can't be used here because a bare
// `String::default()` is `""`, not the real default like `"KEY_RIGHTCTRL"`).

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub hotkey: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            hotkey: "KEY_RIGHTCTRL".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Llm {
    pub model: String,
    /// When true, the LLM may break dictation into multiple lines at natural
    /// boundaries (chat-style message splitting). When false, output stays on a
    /// single line unless the speaker explicitly says "new line"/"new paragraph".
    pub message_splitting: bool,
}

impl Default for Llm {
    fn default() -> Self {
        Self {
            model: "google/gemini-2.0-flash-lite-001".into(),
            message_splitting: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Soniox {
    pub language: String,
}

impl Default for Soniox {
    fn default() -> Self {
        Self {
            language: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub llm: Llm,
    pub soniox: Soniox,
}

impl Config {
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        let home = home_dir().ok_or(ConfigError::NoHome)?;
        Ok(home.join(".config").join("linux-flow").join("config.toml"))
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        let cfg: Self = toml::from_str(&raw)?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        file.write_all(serialized.as_bytes())?;
        Ok(())
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_field_falls_back_to_default_and_keeps_present_values() {
        // `[soniox]` omits `language` and `[llm]` omits `message_splitting`:
        // both fill from their struct default while present values survive.
        let raw = r#"
            [general]
            hotkey = "KEY_F12"

            [llm]
            model = "anthropic/claude"

            [soniox]
        "#;
        let cfg: Config = toml::from_str(raw).expect("missing fields must not fail the parse");
        assert_eq!(cfg.general.hotkey, "KEY_F12");
        assert_eq!(cfg.llm.model, "anthropic/claude");
        assert!(!cfg.llm.message_splitting);
        assert_eq!(cfg.soniox.language, "auto");
    }

    #[test]
    fn missing_table_falls_back_to_default() {
        // A whole missing table (`[soniox]`, `[general]`) defaults entirely.
        let raw = r#"
            [llm]
            model = "x/y"
        "#;
        let cfg: Config = toml::from_str(raw).expect("missing tables must not fail the parse");
        assert_eq!(cfg.general.hotkey, Config::default().general.hotkey);
        assert_eq!(cfg.soniox.language, "auto");
        assert_eq!(cfg.llm.model, "x/y");
    }

    #[test]
    fn empty_document_equals_default() {
        let cfg: Config = toml::from_str("").expect("empty document must parse");
        let def = Config::default();
        assert_eq!(cfg.general.hotkey, def.general.hotkey);
        assert_eq!(cfg.llm.model, def.llm.model);
        assert_eq!(cfg.llm.message_splitting, def.llm.message_splitting);
        assert_eq!(cfg.soniox.language, def.soniox.language);
    }

    #[test]
    fn unknown_field_is_ignored() {
        // A field a newer version writes that this version doesn't know must be
        // ignored, not rejected (forward/backward tolerance, no deny_unknown).
        let raw = r#"
            [general]
            hotkey = "KEY_RIGHTCTRL"

            [llm]
            model = "x/y"
            future_knob = 42

            [soniox]
            language = "auto"
        "#;
        let cfg: Config = toml::from_str(raw).expect("unknown fields must be ignored");
        assert_eq!(cfg.llm.model, "x/y");
    }

    #[test]
    fn default_is_round_trippable() {
        // The default config must serialize and re-parse to itself.
        let serialized = toml::to_string_pretty(&Config::default()).expect("serialize");
        let cfg: Config = toml::from_str(&serialized).expect("re-parse");
        let def = Config::default();
        assert_eq!(cfg.general.hotkey, def.general.hotkey);
        assert_eq!(cfg.llm.model, def.llm.model);
        assert_eq!(cfg.soniox.language, def.soniox.language);
    }
}
