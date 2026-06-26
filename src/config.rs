use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[allow(dead_code)]
pub const DEFAULT_CONFIG_PATH: &str = "/etc/msc/config.toml";
pub const DEFAULT_SERVERS_ROOT: &str = "/opt/minecraft";
pub const DEFAULT_STATE_PATH: &str = "/var/lib/msc/state.json";
#[allow(dead_code)]
pub const DEFAULT_LOG_PATH: &str = "/var/log/msc/msc.log";
pub const DEFAULT_GDRIVE_TOKEN_PATH: &str = "/var/lib/msc/gdrive_token.json";
pub const DEFAULT_TMP_DIR: &str = "/var/lib/msc/tmp";
pub const DEFAULT_WATCHDOG_LOCK: &str = "/var/lib/msc/watchdog.lock";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GlobalConfig {
    #[serde(default = "default_servers_root")]
    pub servers_root: String,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_tmux_socket")]
    pub tmux_socket: String,

    #[serde(default)]
    pub backup: BackupConfig,
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

fn default_servers_root() -> String {
    DEFAULT_SERVERS_ROOT.to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_tmux_socket() -> String {
    "msc".to_string()
}
fn default_gdrive_folder() -> String {
    "MSC Backups".to_string()
}
fn default_token_path() -> String {
    DEFAULT_GDRIVE_TOKEN_PATH.to_string()
}
fn default_tmp_dir() -> String {
    DEFAULT_TMP_DIR.to_string()
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            servers_root: default_servers_root(),
            log_level: default_log_level(),
            tmux_socket: default_tmux_socket(),
            backup: BackupConfig::default(),
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
