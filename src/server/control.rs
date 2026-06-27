use super::config::{parse_memory_str, valid_cpu_list};
use super::{check_start_script_executable, Server};
use crate::config::GlobalConfig;
use crate::error::{AnvilError, Result};
use crate::state::{AppState, DesiredState};
use crate::screen::{get_child_pid, has_child_processes, ScreenClient};
use std::time::{Duration, Instant};

const STOP_GRACE_SECS: u64 = 30;

pub struct ServerController<'a> {
    pub tmux: &'a ScreenClient,
    pub config: &'a GlobalConfig,
}

fn recommended_xmx_mib(mem_max_bytes: u64) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    let bytes = if mem_max_bytes > 3 * GIB / 2 {
        mem_max_bytes - GIB
    } else {
        mem_max_bytes * 3 / 4
    };
    (bytes / (1024 * 1024)).max(64)
}

fn launch_env_prefix(server: &Server) -> String {
    let limits = &server.config.limits;
    let xmx = format!(
        "{}M",
        recommended_xmx_mib(parse_memory_str(&limits.memory_max))
    );
    let pairs = [
        ("ANVIL_SERVER_NAME", server.name.clone()),
        ("ANVIL_SERVER_DIR", server.path.display().to_string()),
        ("ANVIL_MEMORY_MAX", limits.memory_max.clone()),
        ("ANVIL_XMX", xmx),
        ("ANVIL_CPU_CORES", limits.cpu_cores.to_string()),
        (
            "ANVIL_DESCRIPTION",
            server.config.server.description.clone(),
        ),
    ];
    pairs
        .iter()
        .map(|(k, v)| format!("{}='{}' ", k, v.replace('\'', "'\\''")))
        .collect()
}

fn build_launch_command(server: &Server) -> String {
    let limits = &server.config.limits;
    let env = launch_env_prefix(server);
    let affinity = limits.cpu_affinity.as_deref().filter(|a| valid_cpu_list(a));
    match affinity {
        Some(a) => format!("{}taskset -c {} ./start.sh", env, a),
        None => format!("{}./start.sh", env),
    }
}

impl<'a> ServerController<'a> {
    pub fn new(tmux: &'a ScreenClient, config: &'a GlobalConfig) -> Self {
        Self { tmux, config }
    }

    pub fn session_name(server: &Server) -> String {
        server.name.clone()
    }

    pub fn is_online(&self, server: &Server) -> bool {
        let session = Self::session_name(server);
        if !self.tmux.session_exists(&session) {
            return false;
        }
        match self.tmux.get_screen_pid(&session) {
            Some(pid) => has_child_processes(pid),
            None => false,
        }
    }

    pub fn get_server_pid(&self, server: &Server) -> Option<u32> {
        let session = Self::session_name(server);
        let pid = self.tmux.get_screen_pid(&session)?;
        get_child_pid(pid)
    }

    pub fn start(&self, server: &Server, state: &mut AppState) -> Result<u32> {
        let session = Self::session_name(server);

        if self.tmux.session_exists(&session) {
            if self.is_online(server) {
                return Err(AnvilError::ServerAlreadyRunning(server.name.clone()));
            }
            let _ = self.tmux.kill_session(&session);
        }

        check_start_script_executable(server)?;

        let stop_lock = server.path.join("stop.lock");
        if stop_lock.exists() {
            let _ = std::fs::remove_file(&stop_lock);
        }

        let working_dir = server.path.to_str().unwrap_or("/opt/minecraft");
        let start_cmd = build_launch_command(server);
        self.tmux.new_session(&session, working_dir, &start_cmd)?;

        std::thread::sleep(Duration::from_millis(500));

        let pid = self.get_server_pid(server).unwrap_or(0);

        state.set_desired(&server.name, DesiredState::Running);
        let state_path = self.config.state_path();
        let _ = state.save(&state_path);

        Ok(pid)
    }

    pub fn stop(&self, server: &Server, state: &mut AppState) -> Result<()> {
        let session = Self::session_name(server);

        if !self.tmux.session_exists(&session) {
            state.set_desired(&server.name, DesiredState::Stopped);
            let state_path = self.config.state_path();
            let _ = state.save(&state_path);
            return Err(AnvilError::ServerNotRunning(server.name.clone()));
        }

        let stop_lock = server.path.join("stop.lock");
        if let Err(e) = std::fs::write(&stop_lock, "") {
            tracing::warn!(server = %server.name, error = %e, "Failed to create stop.lock");
        }

        tracing::info!(server = %server.name, "Sending stop command");
        let stop_cmd = if server.name.contains("proxy") || server.name.contains("bungee") || server.name.contains("velocity") {
            "end"
        } else {
            "stop"
        };
        let _ = self.tmux.send_keys(&session, stop_cmd);

        let deadline = Instant::now() + Duration::from_secs(STOP_GRACE_SECS);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_secs(1));
            if !self.is_online(server) {
                tracing::info!(server = %server.name, "Server stopped gracefully");
                break;
            }
        }

        if self.is_online(server) {
            tracing::warn!(
                server = %server.name,
                "Server did not stop in {}s, force quitting screen",
                STOP_GRACE_SECS
            );
            let _ = self.tmux.kill_session(&session);
        }

        state.set_desired(&server.name, DesiredState::Stopped);
        let state_path = self.config.state_path();
        let _ = state.save(&state_path);

        tracing::info!(server = %server.name, "Server stopped");
        Ok(())
    }

    pub fn restart(&self, server: &Server, state: &mut AppState) -> Result<u32> {
        if self.is_online(server) {
            if let Err(e) = self.stop(server, state) {
                tracing::warn!(server = %server.name, error = %e, "Error during stop in restart");
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        self.start(server, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::config::ServerConfig;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn server_with_script(dir: &std::path::Path, memory_max: &str, script: &str) -> Server {
        std::fs::write(dir.join("start.sh"), script).unwrap();
        let mut config = ServerConfig::default();
        config.limits.memory_max = memory_max.to_string();
        Server {
            name: "test".to_string(),
            path: PathBuf::from(dir),
            config,
        }
    }

    #[test]
    fn recommended_xmx_leaves_headroom() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert_eq!(recommended_xmx_mib(4 * GIB), 3072);
        assert_eq!(recommended_xmx_mib(8 * GIB), 7168);
        assert_eq!(recommended_xmx_mib(GIB), 768);
        assert_eq!(recommended_xmx_mib(512 * 1024 * 1024), 384);
    }
}
