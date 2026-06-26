use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DesiredState {
    Running,
    Stopped,
}

impl std::fmt::Display for DesiredState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesiredState::Running => write!(f, "running"),
            DesiredState::Stopped => write!(f, "stopped"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerState {
    pub desired: DesiredState,
    pub last_seen: Option<DateTime<Utc>>,
}

impl ServerState {
    #[allow(dead_code)]
    pub fn running() -> Self {
        Self {
            desired: DesiredState::Running,
            last_seen: Some(Utc::now()),
        }
    }

    #[allow(dead_code)]
    pub fn stopped() -> Self {
        Self {
            desired: DesiredState::Stopped,
            last_seen: Some(Utc::now()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppState {
    pub schema_version: u32,
    pub servers: HashMap<String, ServerState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            servers: HashMap::new(),
        }
    }
}

impl AppState {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read state: {}", path.display()))?;
        match serde_json::from_str::<Self>(&content) {
            Ok(state) => Ok(state),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "state.json is invalid, using empty state"
                );
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_file_name(".state.json.tmp");
        let content = serde_json::to_string_pretty(self).context("Failed to serialize state")?;
        std::fs::write(&tmp_path, &content)
            .with_context(|| format!("Failed to write tmp state: {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, path).with_context(|| {
            format!("Failed to atomically rename state file: {}", path.display())
        })?;
        Ok(())
    }

    pub fn set_desired(&mut self, name: &str, desired: DesiredState) {
        let entry = self
            .servers
            .entry(name.to_string())
            .or_insert_with(|| ServerState {
                desired: desired.clone(),
                last_seen: None,
            });
        entry.desired = desired;
        entry.last_seen = Some(Utc::now());
    }

    pub fn get_desired(&self, name: &str) -> Option<&DesiredState> {
        self.servers.get(name).map(|s| &s.desired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        let mut state = AppState::default();
        state.set_desired("lobby", DesiredState::Running);
        state.save(&state_path).unwrap();
        let loaded = AppState::load(&state_path).unwrap();
        assert_eq!(loaded.get_desired("lobby"), Some(&DesiredState::Running));
    }

    #[test]
    fn test_corrupt_json_returns_empty() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{ invalid json }").unwrap();
        let state = AppState::load(&state_path).unwrap();
        assert!(state.servers.is_empty());
    }

    #[test]
    fn test_desired_transitions() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        let mut state = AppState::default();
        state.set_desired("lobby", DesiredState::Running);
        state.save(&state_path).unwrap();
        let mut state2 = AppState::load(&state_path).unwrap();
        state2.set_desired("lobby", DesiredState::Stopped);
        state2.save(&state_path).unwrap();
        let state3 = AppState::load(&state_path).unwrap();
        assert_eq!(state3.get_desired("lobby"), Some(&DesiredState::Stopped));
    }
}
