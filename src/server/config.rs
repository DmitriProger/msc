use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerLimits {
    #[serde(default = "default_memory_max")]
    pub memory_max: String,

    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: u32,

    pub cpu_affinity: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerSettings {
    #[serde(default)]
    pub description: String,

    #[serde(default = "default_true")]
    pub auto_restart: bool,

    #[serde(default = "default_restart_delay")]
    pub restart_delay_secs: u64,

    #[serde(default = "default_max_restart_attempts")]
    pub max_restart_attempts: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BackupServerConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_schedule")]
    pub schedule: String,

    #[serde(default = "default_keep_last")]
    pub keep_last: u32,

    #[serde(default = "default_archive_format")]
    pub archive_format: String,

    #[serde(default = "default_true")]
    pub stop_server: bool,

    #[serde(default)]
    pub include: Vec<String>,

    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ServerConfig {
    #[serde(default)]
    pub limits: ServerLimits,

    #[serde(default)]
    pub server: ServerSettings,

    #[serde(default)]
    pub backup: Option<BackupServerConfig>,
}

fn default_memory_max() -> String {
    "4G".to_string()
}
fn default_cpu_cores() -> u32 {
    2
}
fn default_true() -> bool {
    true
}
fn default_restart_delay() -> u64 {
    5
}
fn default_max_restart_attempts() -> u32 {
    3
}
fn default_schedule() -> String {
    "0 4 * * *".to_string()
}
fn default_keep_last() -> u32 {
    7
}
fn default_archive_format() -> String {
    "zip".to_string()
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            memory_max: default_memory_max(),
            cpu_cores: default_cpu_cores(),
            cpu_affinity: None,
        }
    }
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            description: String::new(),
            auto_restart: true,
            restart_delay_secs: default_restart_delay(),
            max_restart_attempts: default_max_restart_attempts(),
        }
    }
}

impl Default for BackupServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: default_schedule(),
            keep_last: default_keep_last(),
            archive_format: default_archive_format(),
            stop_server: true,
            include: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

impl ServerConfig {
    pub fn load(server_dir: &Path) -> Result<Self> {
        let config_path = server_dir.join("anvil.toml");
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read server config: {}", config_path.display()))?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", config_path.display(), e))?;
        Ok(config)
    }

    pub fn memory_max_bytes(&self) -> u64 {
        parse_memory_str(&self.limits.memory_max)
    }
}

pub fn parse_memory_str(s: &str) -> u64 {
    let s = s.trim();
    if let Some(n) = s.strip_suffix('G') {
        n.parse::<u64>().unwrap_or(4) * 1024 * 1024 * 1024
    } else if let Some(n) = s.strip_suffix('M') {
        n.parse::<u64>().unwrap_or(4096) * 1024 * 1024
    } else if let Some(n) = s.strip_suffix('K') {
        n.parse::<u64>().unwrap_or(0) * 1024
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_str() {
        assert_eq!(parse_memory_str("4G"), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_memory_str("512M"), 512 * 1024 * 1024);
        assert_eq!(parse_memory_str("1024K"), 1024 * 1024);
    }

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.limits.memory_max, "4G");
        assert!(config.server.auto_restart);
        assert_eq!(config.server.max_restart_attempts, 3);
    }
}
