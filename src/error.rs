#![allow(dead_code)]
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnvilError {
    #[error("Server '{0}' not found")]
    ServerNotFound(String),

    #[error("Server '{0}' is already running")]
    ServerAlreadyRunning(String),

    #[error("Server '{0}' is not running")]
    ServerNotRunning(String),

    #[error("Invalid server name '{0}': must match ^[a-z0-9_-]{{1,64}}$")]
    InvalidServerName(String),

    #[error("Path traversal detected: '{0}'")]
    PathTraversal(String),

    #[error("start.sh not found in server directory '{0}'")]
    StartScriptMissing(String),

    #[error("start.sh is not executable in server directory '{0}'")]
    StartScriptNotExecutable(String),

    #[error("tmux is not installed. Install it with: apt install tmux")]
    TmuxNotInstalled,

    #[error("tmux session '{0}' not found")]
    TmuxSessionNotFound(String),

    #[error("tmux command failed: {0}")]
    TmuxCommandFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Timeout waiting for server '{0}' to stop")]
    StopTimeout(String),

    #[error("Server '{0}' failed to start after {1} attempts")]
    MaxRestartsExceeded(String, u32),

    #[error("Backup error: {0}")]
    Backup(String),

    #[error("Google Drive error: {0}")]
    GoogleDrive(String),

    #[error("OAuth error: {0}")]
    OAuth(String),
}

pub type Result<T> = std::result::Result<T, AnvilError>;
