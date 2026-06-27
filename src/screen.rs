use crate::error::{AnvilError, Result};
use std::process::Command;

pub struct ScreenClient;

impl ScreenClient {
    pub fn new(_socket: &str) -> Self {
        Self
    }

    pub fn check_installed() -> Result<()> {
        let result = Command::new("screen").arg("-v").output();
        match result {
            Ok(output) if output.status.success() => Ok(()),
            _ => Err(AnvilError::TmuxNotInstalled),
        }
    }

    pub fn session_exists(&self, session: &str) -> bool {
        let output = Command::new("screen").arg("-list").output();
        if let Ok(out) = output {
            let list = String::from_utf8_lossy(&out.stdout);
            list.contains(&format!(".{}", session))
        } else {
            false
        }
    }

    pub fn new_session(&self, session: &str, working_dir: &str, cmd_str: &str) -> Result<()> {
        let status = Command::new("screen")
            .current_dir(working_dir)
            .args(["-dmS", session])
            .arg("bash")
            .arg("-c")
            .arg(cmd_str)
            .status()
            .map_err(|e| AnvilError::TmuxCommandFailed(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(AnvilError::TmuxCommandFailed("Failed to create screen session".to_string()))
        }
    }

    pub fn kill_session(&self, session: &str) -> Result<()> {
        let _ = Command::new("screen")
            .args(["-XS", session, "quit"])
            .status();
        Ok(())
    }

    pub fn send_keys(&self, session: &str, keys: &str) -> Result<()> {
        let cmd = format!("{}\r", keys);
        let status = Command::new("screen")
            .args(["-S", session, "-p", "0", "-X", "stuff", &cmd])
            .status()
            .map_err(|e| AnvilError::TmuxCommandFailed(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(AnvilError::TmuxCommandFailed("Failed to send keys to screen".to_string()))
        }
    }

    pub fn attach_session(&self, session: &str) -> Result<std::process::ExitStatus> {
        let status = Command::new("screen")
            .arg("-r")
            .arg(session)
            .status()
            .map_err(|e| AnvilError::TmuxCommandFailed(e.to_string()))?;
        Ok(status)
    }

    pub fn get_screen_pid(&self, session: &str) -> Option<u32> {
        let output = Command::new("screen").arg("-list").output().ok()?;
        let list = String::from_utf8_lossy(&output.stdout);
        for line in list.lines() {
            if line.contains(&format!(".{}", session)) {
                let part = line.split_whitespace().next()?;
                let pid_str = part.split('.').next()?;
                return pid_str.parse::<u32>().ok();
            }
        }
        None
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

    if let Ok(output) = Command::new("pgrep")
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
