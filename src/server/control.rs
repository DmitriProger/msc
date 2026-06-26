use super::{check_start_script_executable, Server};
use crate::config::GlobalConfig;
use crate::error::{MscError, Result};
use crate::state::{AppState, DesiredState};
use crate::tmux::{get_child_pid, has_child_processes, TmuxClient};
use std::time::{Duration, Instant};

pub struct ServerController<'a> {
    pub tmux: &'a TmuxClient,
    pub config: &'a GlobalConfig,
}

impl<'a> ServerController<'a> {
    pub fn new(tmux: &'a TmuxClient, config: &'a GlobalConfig) -> Self {
        Self { tmux, config }
    }

    pub fn session_name(server: &Server) -> String {
        format!("msc_{}", server.name)
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
        get_child_pid(pane_pid).or(Some(pane_pid))
    }

    pub fn start(&self, server: &Server, state: &mut AppState) -> Result<u32> {
        let session = Self::session_name(server);

        if self.tmux.session_exists(&session) {
            if self.is_online(server) {
                return Err(MscError::ServerAlreadyRunning(server.name.clone()));
            }
            // session exists but process died — kill stale session
            if let Err(e) = self.tmux.kill_session(&session) {
                tracing::warn!(server = %server.name, error = %e, "Failed to kill stale tmux session");
            }
        }

        check_start_script_executable(server)?;

        let working_dir = server.path.to_str().unwrap_or("/opt/minecraft");
        self.tmux.new_session(&session, working_dir)?;
        tracing::trace!(server = %server.name, session = %session, "Created tmux session");

        let start_cmd = if let Some(affinity) = &server.config.limits.cpu_affinity {
            format!("taskset -c {} ./start.sh", affinity)
        } else {
            "./start.sh".to_string()
        };

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
            return Err(MscError::ServerNotRunning(server.name.clone()));
        }

        tracing::info!(server = %server.name, "Sending stop command");
        if let Err(e) = self.tmux.send_keys(&session, "stop") {
            tracing::warn!(server = %server.name, error = %e, "Failed to send stop command");
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_secs(1));
            if !self.is_online(server) {
                tracing::info!(server = %server.name, "Server stopped gracefully");
                break;
            }
        }

        if self.is_online(server) {
            tracing::warn!(server = %server.name, "Server did not stop in 30s, sending SIGTERM");
            self.send_signal(server, nix::sys::signal::Signal::SIGTERM);
            std::thread::sleep(Duration::from_secs(10));
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
