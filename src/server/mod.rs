pub mod config;
pub mod control;
pub mod metrics;

use crate::config::GlobalConfig;
use crate::error::{AnvilError, Result};
use config::ServerConfig;
use regex::Regex;
use std::path::PathBuf;
use std::sync::OnceLock;

static SERVER_NAME_RE: OnceLock<Regex> = OnceLock::new();

fn server_name_regex() -> &'static Regex {
    SERVER_NAME_RE.get_or_init(|| Regex::new(r"^[a-z0-9_-]{1,64}$").unwrap())
}

#[derive(Debug, Clone)]
pub struct Server {
    pub name: String,
    pub path: PathBuf,
    pub config: ServerConfig,
}

impl Server {
    pub fn start_script(&self) -> PathBuf {
        self.path.join("start.sh")
    }

    #[allow(dead_code)]
    pub fn tmux_session_name(&self, socket: &str) -> String {
        let _ = socket;
        format!("anvil_{}", self.name)
    }
}

pub fn validate_server_name(name: &str) -> Result<()> {
    if !server_name_regex().is_match(name) {
        return Err(AnvilError::InvalidServerName(name.to_string()));
    }
    Ok(())
}

pub fn discover_servers(global_config: &GlobalConfig) -> Vec<Server> {
    let root = global_config.servers_root_path();
    let mut servers = Vec::new();

    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(path = %root.display(), error = %e, "Cannot read servers_root");
            return servers;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if !server_name_regex().is_match(&name) {
            tracing::warn!(name = %name, "Ignoring directory with invalid server name");
            continue;
        }

        if !validate_path_within_root(&path, &root) {
            tracing::warn!(path = %path.display(), "Path traversal detected, skipping");
            continue;
        }

        let start_sh = path.join("start.sh");
        if !start_sh.exists() {
            tracing::debug!(server = %name, "Skipping directory without start.sh");
            continue;
        }

        let config = match ServerConfig::load(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(server = %name, error = %e, "Failed to load server config, using defaults");
                ServerConfig::default()
            }
        };

        servers.push(Server { name, path, config });
    }

    servers.sort_by(|a, b| a.name.cmp(&b.name));
    servers
}

pub fn find_server<'a>(servers: &'a [Server], name: &str) -> Result<&'a Server> {
    validate_server_name(name)?;
    servers
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| AnvilError::ServerNotFound(name.to_string()))
}

pub fn validate_path_within_root(path: &std::path::Path, root: &std::path::Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(canonical_path), Ok(canonical_root)) => canonical_path.starts_with(&canonical_root),
        _ => false,
    }
}

pub fn check_start_script_executable(server: &Server) -> Result<()> {
    let start_sh = server.start_script();
    if !start_sh.exists() {
        return Err(AnvilError::StartScriptMissing(
            server.path.display().to_string(),
        ));
    }
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(&start_sh)?;
    let mode = metadata.permissions().mode();
    if mode & 0o111 == 0 {
        return Err(AnvilError::StartScriptNotExecutable(
            server.path.display().to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_global_config(root: &str) -> GlobalConfig {
        GlobalConfig {
            servers_root: root.to_string(),
            log_level: "info".to_string(),
            tmux_socket: "anvil".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_discover_no_start_sh() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("myserver")).unwrap();
        let config = make_global_config(dir.path().to_str().unwrap());
        let servers = discover_servers(&config);
        assert!(servers.is_empty());
    }

    #[test]
    fn test_discover_with_start_sh() {
        let dir = tempdir().unwrap();
        let srv = dir.path().join("lobby");
        std::fs::create_dir(&srv).unwrap();
        std::fs::write(srv.join("start.sh"), "#!/bin/bash\necho hi").unwrap();
        let config = make_global_config(dir.path().to_str().unwrap());
        let servers = discover_servers(&config);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "lobby");
    }

    #[test]
    fn test_invalid_server_name_space() {
        assert!(validate_server_name("my server").is_err());
    }

    #[test]
    fn test_valid_server_name() {
        assert!(validate_server_name("lobby-01").is_ok());
        assert!(validate_server_name("survival_v2").is_ok());
    }

    #[test]
    fn test_invalid_server_name_ignored_in_discover() {
        let dir = tempdir().unwrap();
        let srv = dir.path().join("My Server");
        std::fs::create_dir(&srv).unwrap();
        std::fs::write(srv.join("start.sh"), "#!/bin/bash\n").unwrap();
        let config = make_global_config(dir.path().to_str().unwrap());
        let servers = discover_servers(&config);
        assert!(servers.is_empty());
    }
}
