use crate::error::{AnvilError, Result};
use std::process::Command;

pub struct TmuxClient {
    pub socket: String,
}

impl TmuxClient {
    pub fn new(socket: &str) -> Self {
        Self {
            socket: socket.to_string(),
        }
    }

    fn base_args(&self) -> Vec<String> {
        vec!["-L".to_string(), self.socket.clone()]
    }

    pub fn check_installed() -> Result<()> {
        let result = Command::new("tmux").arg("-V").output();
        match result {
            Ok(output) if output.status.success() => Ok(()),
            _ => Err(AnvilError::TmuxNotInstalled),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new("tmux");
        for a in self.base_args() {
            cmd.arg(a);
        }
        for a in args {
            cmd.arg(a);
        }
        let output = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AnvilError::TmuxNotInstalled
            } else {
                AnvilError::TmuxCommandFailed(e.to_string())
            }
        })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(AnvilError::TmuxCommandFailed(stderr))
        }
    }

    fn run_allow_fail(&self, args: &[&str]) -> std::result::Result<String, String> {
        let mut cmd = Command::new("tmux");
        for a in self.base_args() {
            cmd.arg(a);
        }
        for a in args {
            cmd.arg(a);
        }
        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn session_exists(&self, session: &str) -> bool {
        self.run_allow_fail(&["has-session", "-t", session]).is_ok()
    }

    pub fn new_session(&self, session: &str, working_dir: &str) -> Result<()> {
        self.run(&["new-session", "-d", "-s", session, "-c", working_dir])?;
        // Enable mouse support to allow scroll wheel log navigation
        let _ = self.run(&["set-option", "-t", session, "mouse", "on"]);
        // Bind Ctrl+X to detach from the tmux session without needing the prefix
        let _ = self.run(&["bind-key", "-n", "C-x", "detach-client"]);
        Ok(())
    }

    pub fn kill_session(&self, session: &str) -> Result<()> {
        match self.run(&["kill-session", "-t", session]) {
            Ok(_) => Ok(()),
            Err(AnvilError::TmuxCommandFailed(e)) if e.contains("can't find session") => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn send_keys(&self, session: &str, keys: &str) -> Result<()> {
        self.run(&["send-keys", "-t", session, keys, "Enter"])?;
        Ok(())
    }

    pub fn pane_pid(&self, session: &str) -> Result<u32> {
        let pid_str = self.run(&["display-message", "-p", "-t", session, "#{pane_pid}"])?;
        pid_str.trim().parse::<u32>().map_err(|_| {
            AnvilError::TmuxCommandFailed(format!("Invalid PID from tmux: '{}'", pid_str))
        })
    }

    pub fn run_in_session(&self, session: &str, command: &str) -> Result<()> {
        self.run(&["send-keys", "-t", session, command, "Enter"])?;
        Ok(())
    }

    pub fn attach_session(&self, session: &str) -> Result<std::process::ExitStatus> {
        let status = Command::new("tmux")
            .args(self.base_args())
            .arg("attach-session")
            .arg("-t")
            .arg(session)
            .status()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AnvilError::TmuxNotInstalled
                } else {
                    AnvilError::TmuxCommandFailed(e.to_string())
                }
            })?;
        Ok(status)
    }

    #[allow(dead_code)]
    pub fn list_sessions(&self) -> Vec<String> {
        match self.run_allow_fail(&["list-sessions", "-F", "#{session_name}"]) {
            Ok(output) => output.lines().map(|l| l.to_string()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

pub fn has_child_processes(pane_pid: u32) -> bool {
    !descendant_pids(pane_pid).is_empty()
}

pub fn get_child_pid(pane_pid: u32) -> Option<u32> {
    descendant_pids(pane_pid).last().copied()
}

fn descendant_pids(pid: u32) -> Vec<u32> {
    let mut descendants = Vec::new();
    collect_descendants(pid, &mut descendants);
    descendants
}

fn collect_descendants(pid: u32, descendants: &mut Vec<u32>) {
    for child in direct_child_pids(pid) {
        descendants.push(child);
        collect_descendants(child, descendants);
    }
}

fn direct_child_pids(pid: u32) -> Vec<u32> {
    let children_path = format!("/proc/{}/task/{}/children", pid, pid);
    if let Ok(content) = std::fs::read_to_string(&children_path) {
        let pids: Vec<u32> = content
            .split_whitespace()
            .filter_map(|pid| pid.parse::<u32>().ok())
            .collect();
        if !pids.is_empty() {
            return pids;
        }
    }

    // Fallback for macOS or Linux systems without proc children support
    if let Ok(output) = std::process::Command::new("pgrep")
        .arg("-P")
        .arg(pid.to_string())
        .output()
    {
        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout);
            let pids: Vec<u32> = content
                .split_whitespace()
                .filter_map(|p| p.parse::<u32>().ok())
                .collect();
            return pids;
        }
    }

    Vec::new()
}
