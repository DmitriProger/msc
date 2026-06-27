use super::config::{parse_jvm_mem, parse_memory_str, valid_cpu_list};
use super::{check_start_script_executable, Server};
use crate::config::GlobalConfig;
use crate::error::{AnvilError, Result};
use crate::server::metrics::format_bytes;
use crate::state::{AppState, DesiredState};
use crate::tmux::{get_child_pid, has_child_processes, TmuxClient};
use std::time::{Duration, Instant};

/// How long to wait for a graceful in-game `stop` before escalating to signals.
const STOP_GRACE_SECS: u64 = 60;
/// How long to wait after SIGTERM before sending SIGKILL.
const SIGTERM_WAIT_SECS: u64 = 10;

pub struct ServerController<'a> {
    pub tmux: &'a TmuxClient,
    pub config: &'a GlobalConfig,
}

/// Effective UID of the current process (0 = root). Used to pick between a
/// system scope and a `--user` scope for `systemd-run`.
fn effective_uid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

/// Is `systemd-run` available and functional so we can enforce real cgroup resource limits?
fn systemd_run_available() -> bool {
    use std::sync::OnceLock;
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let version_ok = std::process::Command::new("systemd-run")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !version_ok {
            return false;
        }

        // Try a dry-run check to see if we can actually create a transient scope.
        // Set SYSTEMD_ASK_PASSWORD=0 to fail immediately if polkit auth is required
        // instead of hanging the terminal.
        let is_root = effective_uid() == 0;
        let mut cmd = std::process::Command::new("systemd-run");
        cmd.env("SYSTEMD_ASK_PASSWORD", "0");
        cmd.arg("--scope");
        if !is_root {
            cmd.arg("--user");
        }
        cmd.arg("true");

        cmd.output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Single-quote a value for safe interpolation into the shell launch line.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Recommended JVM heap (MiB) for a given `memory_max` ceiling: leave ~1 GiB of
/// headroom for non-heap JVM memory, or 75% when the limit is small. Exposed via
/// the `ANVIL_XMX` env var so a start.sh can stay in sync with the cgroup wall.
fn recommended_xmx_mib(mem_max_bytes: u64) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    let bytes = if mem_max_bytes > 3 * GIB / 2 {
        mem_max_bytes - GIB
    } else {
        mem_max_bytes * 3 / 4
    };
    (bytes / (1024 * 1024)).max(64)
}

/// Build the `ANVIL_*` environment prefix exported into the launch. These are
/// opt-in: a start.sh may reference `${ANVIL_XMX}` etc., or ignore them entirely.
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
        .map(|(k, v)| format!("{}={} ", k, shell_quote(v)))
        .collect()
}

/// Build the command (typed into the tmux pane) that launches the server.
///
/// When `systemd-run` is available, the server runs inside a transient cgroup
/// v2 scope with real `MemoryMax`/`MemoryHigh`/`CPUQuota`/`AllowedCPUs` limits
/// enforced by the kernel — independent of any `-Xmx` flags in start.sh.
/// Otherwise it degrades gracefully to `taskset` (affinity only) or a plain
/// launch, logging that limits are NOT enforced rather than failing to start.
/// `ANVIL_*` env vars are exported in all cases for start.sh to opt into.
fn build_launch_command(server: &Server) -> String {
    let limits = &server.config.limits;
    let env = launch_env_prefix(server);

    // Validate affinity once; ignore (with a warning) if it isn't a clean CPU list.
    let affinity = limits.cpu_affinity.as_deref().filter(|a| valid_cpu_list(a));
    if limits.cpu_affinity.is_some() && affinity.is_none() {
        tracing::warn!(
            server = %server.name,
            value = ?limits.cpu_affinity,
            "Ignoring invalid cpu_affinity (must be a CPU list like 0,1 or 3-6)"
        );
    }

    if systemd_run_available() {
        let mem_max = parse_memory_str(&limits.memory_max);
        let mem_high = (mem_max as f64 * 0.85) as u64;
        let cpu_quota = limits.cpu_cores.max(1) * 100;
        let scope_flags = if effective_uid() == 0 {
            "--scope"
        } else {
            "--scope --user"
        };
        let mut cmd = format!(
            "{env}systemd-run {flags} --quiet --collect --unit=anvil-{name} \
             -p MemoryMax={mem_max} -p MemoryHigh={mem_high} -p CPUQuota={quota}%",
            env = env,
            flags = scope_flags,
            name = server.name,
            mem_max = mem_max,
            mem_high = mem_high,
            quota = cpu_quota,
        );
        if let Some(a) = affinity {
            cmd.push_str(&format!(" -p AllowedCPUs={}", a));
        }
        cmd.push_str(" ./start.sh");
        cmd
    } else {
        tracing::warn!(
            server = %server.name,
            "systemd-run unavailable; resource limits (RAM/CPU) are NOT enforced"
        );
        match affinity {
            Some(a) => format!("{}taskset -c {} ./start.sh", env, a),
            None => format!("{}./start.sh", env),
        }
    }
}

/// Returns a human-readable warning when start.sh requests a `-Xmx` heap larger
/// than the configured `memory_max` cgroup ceiling (which would get the JVM
/// OOM-killed). Returns `None` when there's no conflict or no `-Xmx` present.
pub fn xmx_warning(server: &Server) -> Option<String> {
    let content = std::fs::read_to_string(server.start_script()).ok()?;
    let xmx = content
        .split_whitespace()
        .find_map(|tok| {
            tok.trim_matches('"')
                .trim_matches('\'')
                .strip_prefix("-Xmx")
        })
        .map(parse_jvm_mem)
        .filter(|&n| n > 0)?;
    let limit = parse_memory_str(&server.config.limits.memory_max);
    if limit > 0 && xmx > limit {
        Some(format!(
            "start.sh sets -Xmx={} which exceeds memory_max={} ({}); \
             the JVM may be OOM-killed by the cgroup limit. Lower -Xmx below memory_max (leave ~1G headroom).",
            format_bytes(xmx),
            server.config.limits.memory_max,
            format_bytes(limit),
        ))
    } else {
        None
    }
}

impl<'a> ServerController<'a> {
    pub fn new(tmux: &'a TmuxClient, config: &'a GlobalConfig) -> Self {
        Self { tmux, config }
    }

    pub fn session_name(server: &Server) -> String {
        format!("anvil_{}", server.name)
    }

    pub fn is_online(&self, server: &Server) -> bool {
        let session = Self::session_name(server);
        if !self.tmux.session_exists(&session) {
            return false;
        }
        match self.tmux.pane_pid(&session) {
            Ok(pane_pid) => has_child_processes(pane_pid),
            Err(_) => false,
        }
    }

    pub fn get_server_pid(&self, server: &Server) -> Option<u32> {
        let session = Self::session_name(server);
        if !self.tmux.session_exists(&session) {
            return None;
        }
        let pane_pid = self.tmux.pane_pid(&session).ok()?;
        get_child_pid(pane_pid)
    }

    pub fn start(&self, server: &Server, state: &mut AppState) -> Result<u32> {
        let session = Self::session_name(server);

        if self.tmux.session_exists(&session) {
            if self.is_online(server) {
                return Err(AnvilError::ServerAlreadyRunning(server.name.clone()));
            }
            // Session exists but process died; kill stale session.
            if let Err(e) = self.tmux.kill_session(&session) {
                tracing::warn!(server = %server.name, error = %e, "Failed to kill stale tmux session");
            }
        }

        check_start_script_executable(server)?;

        let working_dir = server.path.to_str().unwrap_or("/opt/minecraft");
        self.tmux.new_session(&session, working_dir)?;
        tracing::trace!(server = %server.name, session = %session, "Created tmux session");

        if let Some(warning) = xmx_warning(server) {
            tracing::warn!(server = %server.name, "{}", warning);
        }

        let start_cmd = build_launch_command(server);
        self.tmux.run_in_session(&session, &start_cmd)?;
        tracing::info!(server = %server.name, "Server start command sent");

        std::thread::sleep(Duration::from_millis(500));

        let pid = self.get_server_pid(server).unwrap_or(0);

        state.set_desired(&server.name, DesiredState::Running);
        let state_path = self.config.state_path();
        if let Err(e) = state.save(&state_path) {
            tracing::warn!(error = %e, "Failed to save state after start");
        }

        Ok(pid)
    }

    pub fn stop(&self, server: &Server, state: &mut AppState) -> Result<()> {
        let session = Self::session_name(server);

        if !self.tmux.session_exists(&session) {
            state.set_desired(&server.name, DesiredState::Stopped);
            let state_path = self.config.state_path();
            if let Err(e) = state.save(&state_path) {
                tracing::warn!(error = %e, "Failed to save state");
            }
            return Err(AnvilError::ServerNotRunning(server.name.clone()));
        }

        tracing::info!(server = %server.name, "Sending stop command");
        if let Err(e) = self.tmux.send_keys(&session, "stop") {
            tracing::warn!(server = %server.name, error = %e, "Failed to send stop command");
        }

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
                "Server did not stop in {}s, sending SIGTERM",
                STOP_GRACE_SECS
            );
            self.send_signal(server, nix::sys::signal::Signal::SIGTERM);
            std::thread::sleep(Duration::from_secs(SIGTERM_WAIT_SECS));
        }

        if self.is_online(server) {
            tracing::warn!(server = %server.name, "Server still alive after SIGTERM, sending SIGKILL");
            self.send_signal(server, nix::sys::signal::Signal::SIGKILL);
        }

        if let Err(e) = self.tmux.kill_session(&session) {
            tracing::warn!(server = %server.name, error = %e, "Failed to kill tmux session");
        }

        state.set_desired(&server.name, DesiredState::Stopped);
        let state_path = self.config.state_path();
        if let Err(e) = state.save(&state_path) {
            tracing::warn!(error = %e, "Failed to save state after stop");
        }

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

    fn send_signal(&self, server: &Server, signal: nix::sys::signal::Signal) {
        if let Some(pid) = self.get_server_pid(server) {
            let pid = nix::unistd::Pid::from_raw(pid as i32);
            if let Err(e) = nix::sys::signal::kill(pid, signal) {
                tracing::warn!(
                    server = %server.name,
                    signal = ?signal,
                    error = %e,
                    "Failed to send signal"
                );
            }
        }
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
    fn xmx_warning_fires_when_heap_exceeds_limit() {
        let dir = tempdir().unwrap();
        let server = server_with_script(
            dir.path(),
            "4G",
            "#!/bin/bash\njava -Xms1G -Xmx8G -jar server.jar nogui\n",
        );
        assert!(xmx_warning(&server).is_some());
    }

    #[test]
    fn xmx_warning_silent_when_heap_within_limit() {
        let dir = tempdir().unwrap();
        let server = server_with_script(
            dir.path(),
            "8G",
            "#!/bin/bash\njava -Xms1G -Xmx6G -jar server.jar nogui\n",
        );
        assert!(xmx_warning(&server).is_none());
    }

    #[test]
    fn xmx_warning_silent_without_xmx() {
        let dir = tempdir().unwrap();
        let server = server_with_script(dir.path(), "4G", "#!/bin/bash\n./run.sh\n");
        assert!(xmx_warning(&server).is_none());
    }

    #[test]
    fn recommended_xmx_leaves_headroom() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert_eq!(recommended_xmx_mib(4 * GIB), 3072); // 4G - 1G
        assert_eq!(recommended_xmx_mib(8 * GIB), 7168); // 8G - 1G
        assert_eq!(recommended_xmx_mib(GIB), 768); // small -> 75%
        assert_eq!(recommended_xmx_mib(512 * 1024 * 1024), 384); // 512M -> 75%
    }

    #[test]
    fn shell_quote_escapes_quotes_and_spaces() {
        assert_eq!(shell_quote("Лобби сервер"), "'Лобби сервер'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("4G"), "'4G'");
    }

    #[test]
    fn launch_env_prefix_contains_anvil_vars() {
        let dir = tempdir().unwrap();
        let mut server = server_with_script(dir.path(), "4G", "#!/bin/bash\n");
        server.config.server.description = "Lobby".to_string();
        let env = launch_env_prefix(&server);
        assert!(env.contains("ANVIL_XMX='3072M'"));
        assert!(env.contains("ANVIL_MEMORY_MAX='4G'"));
        assert!(env.contains("ANVIL_DESCRIPTION='Lobby'"));
        assert!(env.contains("ANVIL_SERVER_NAME='test'"));
    }
}
