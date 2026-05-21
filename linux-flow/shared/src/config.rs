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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct General {
    pub hotkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Llm {
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Soniox {
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ui {
    pub show_tray: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: General,
    pub llm: Llm,
    pub soniox: Soniox,
    pub ui: Ui,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General { hotkey: "KEY_RIGHTCTRL".into() },
            llm: Llm { model: "google/gemini-3.1-flash-lite".into() },
            soniox: Soniox { language: "auto".into() },
            ui: Ui { show_tray: true },
        }
    }
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
