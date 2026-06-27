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

/// Validate a CPU list string as accepted by `taskset -c` / cgroup `AllowedCPUs`,
/// e.g. "0", "0,1", "3-6", "0,2-4". Rejects anything else so the value can be
/// safely interpolated into a launch command (prevents shell injection).
pub fn valid_cpu_list(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    s.split(',').all(|part| {
        let part = part.trim();
        if part.is_empty() {
            return false;
        }
        match part.split_once('-') {
            Some((a, b)) => {
                !a.is_empty()
                    && !b.is_empty()
                    && a.chars().all(|c| c.is_ascii_digit())
                    && b.chars().all(|c| c.is_ascii_digit())
            }
            None => part.chars().all(|c| c.is_ascii_digit()),
        }
    })
}

/// Parse a JVM-style memory size (e.g. "-Xmx" value "14336M", "4g", "512000")
/// into bytes. Accepts upper/lowercase g/m/k suffixes; bare numbers are bytes.
pub fn parse_jvm_mem(s: &str) -> u64 {
    let s = s.trim().trim_matches('"').trim_matches('\'');
    let (num, mult) = match s.chars().last() {
        Some('g') | Some('G') => (&s[..s.len().saturating_sub(1)], 1024u64 * 1024 * 1024),
        Some('m') | Some('M') => (&s[..s.len().saturating_sub(1)], 1024 * 1024),
        Some('k') | Some('K') => (&s[..s.len().saturating_sub(1)], 1024),
        _ => (s, 1),
    };
    num.trim().parse::<u64>().map(|n| n * mult).unwrap_or(0)
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
    fn test_valid_cpu_list() {
        assert!(valid_cpu_list("0"));
        assert!(valid_cpu_list("0,1"));
        assert!(valid_cpu_list("3-6"));
        assert!(valid_cpu_list("0,2-4,7"));
        assert!(!valid_cpu_list(""));
        assert!(!valid_cpu_list("0; rm -rf /"));
        assert!(!valid_cpu_list("0,"));
        assert!(!valid_cpu_list("a-b"));
        assert!(!valid_cpu_list("1 2"));
    }

    #[test]
    fn test_parse_jvm_mem() {
        assert_eq!(parse_jvm_mem("4G"), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_jvm_mem("14336M"), 14336 * 1024 * 1024);
        assert_eq!(parse_jvm_mem("512k"), 512 * 1024);
        assert_eq!(parse_jvm_mem("\"2g\""), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_jvm_mem("1048576"), 1048576);
    }

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.limits.memory_max, "4G");
        assert!(config.server.auto_restart);
        assert_eq!(config.server.max_restart_attempts, 3);
    }
}
