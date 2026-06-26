use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[allow(dead_code)]
pub const DEFAULT_CONFIG_PATH: &str = "/etc/anvil/config.toml";
pub const DEFAULT_SERVERS_ROOT: &str = "/opt/minecraft";
pub const DEFAULT_STATE_PATH: &str = "/var/lib/anvil/state.json";
#[allow(dead_code)]
pub const DEFAULT_LOG_PATH: &str = "/var/log/anvil/anvil.log";
pub const DEFAULT_GDRIVE_TOKEN_PATH: &str = "/var/lib/anvil/gdrive_token.json";
pub const DEFAULT_TMP_DIR: &str = "/var/lib/anvil/tmp";
pub const DEFAULT_WATCHDOG_LOCK: &str = "/var/lib/anvil/watchdog.lock";
pub const DEFAULT_UPDATE_REPO: &str = "DmitriProger/anvil";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[serde(alias = "eng", alias = "english")]
    #[default]
    En,
    #[serde(alias = "rus", alias = "russian")]
    Ru,
}

impl Language {
    pub fn choose<'a>(&self, en: &'a str, ru: &'a str) -> &'a str {
        match self {
            Self::En => en,
            Self::Ru => ru,
        }
    }

    pub fn status_text(&self, online: bool) -> &'static str {
        match (self, online) {
            (Self::En, true) => "ONLINE",
            (Self::En, false) => "OFFLINE",
            (Self::Ru, true) => "ЗАПУЩЕН",
            (Self::Ru, false) => "ВЫКЛЮЧЕН",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GlobalConfig {
    #[serde(default)]
    pub language: Language,

    #[serde(default = "default_servers_root")]
    pub servers_root: String,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_tmux_socket")]
    pub tmux_socket: String,

    #[serde(default)]
    pub backup: BackupConfig,

    #[serde(default)]
    pub update: UpdateConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct BackupConfig {
    #[serde(default = "default_gdrive_folder")]
    pub gdrive_folder: String,

    #[serde(default = "default_token_path")]
    pub token_path: String,

    #[serde(default = "default_tmp_dir")]
    pub tmp_dir: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateConfig {
    #[serde(default = "default_update_repo")]
    pub repo: String,
}

fn default_servers_root() -> String {
    DEFAULT_SERVERS_ROOT.to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_tmux_socket() -> String {
    "anvil".to_string()
}
fn default_gdrive_folder() -> String {
    "Anvil Backups".to_string()
}
fn default_token_path() -> String {
    DEFAULT_GDRIVE_TOKEN_PATH.to_string()
}
fn default_tmp_dir() -> String {
    DEFAULT_TMP_DIR.to_string()
}
fn default_update_repo() -> String {
    DEFAULT_UPDATE_REPO.to_string()
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            repo: default_update_repo(),
        }
    }
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            language: Language::default(),
            servers_root: default_servers_root(),
            log_level: default_log_level(),
            tmux_socket: default_tmux_socket(),
            backup: BackupConfig::default(),
            update: UpdateConfig::default(),
        }
    }
}

impl GlobalConfig {
    pub fn load() -> Result<Self> {
        Self::load_from(DEFAULT_CONFIG_PATH)
    }

    pub fn load_from(path: &str) -> Result<Self> {
        let config_path = PathBuf::from(path);
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", config_path.display()))?;
        Ok(config)
    }

    pub fn servers_root_path(&self) -> PathBuf {
        PathBuf::from(&self.servers_root)
    }

    pub fn state_path(&self) -> PathBuf {
        PathBuf::from(DEFAULT_STATE_PATH)
    }

    #[allow(dead_code)]
    pub fn log_path(&self) -> PathBuf {
        PathBuf::from(DEFAULT_LOG_PATH)
    }

    pub fn watchdog_lock_path(&self) -> PathBuf {
        PathBuf::from(DEFAULT_WATCHDOG_LOCK)
    }

    pub fn save_default(path: &str) -> Result<()> {
        let config = Self::default();
        let content =
            toml::to_string_pretty(&config).context("Failed to serialize default config")?;
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write config: {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_config_uses_english_and_default_repo() {
        let config = GlobalConfig::default();
        assert_eq!(config.language, Language::En);
        assert_eq!(config.update.repo, DEFAULT_UPDATE_REPO);
    }

    #[test]
    fn load_config_accepts_ru_language_and_update_repo() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
language = "ru"
servers_root = "/srv/minecraft"

[update]
repo = "owner/project"
"#,
        )
        .unwrap();

        let config = GlobalConfig::load_from(path.to_str().unwrap()).unwrap();
        assert_eq!(config.language, Language::Ru);
        assert_eq!(config.servers_root, "/srv/minecraft");
        assert_eq!(config.update.repo, "owner/project");
    }

    #[test]
    fn load_config_rejects_unknown_language() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, r#"language = "de""#).unwrap();

        let err = GlobalConfig::load_from(path.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("Failed to parse config"));
    }
}
