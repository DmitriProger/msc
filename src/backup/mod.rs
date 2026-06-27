pub mod archive;
pub mod gdrive;
pub mod oauth;
pub mod restore;
pub mod scheduler;

use crate::config::GlobalConfig;
use crate::server::control::ServerController;
use crate::server::Server;
use crate::state::AppState;
use crate::tmux::TmuxClient;
use anyhow::{Context, Result};
use archive::{create_zip_archive, ArchiveConfig};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Directory where a server's backup archives live.
pub fn backup_dir(server: &Server) -> PathBuf {
    server.path.join("backups")
}

/// Create a backup archive of a server and return its path.
///
/// Shared by the `anvil <name> backup` CLI command and the watchdog scheduler.
/// Online servers are either stopped+restarted (`stop_server = true`) or hot
/// backed up via `save-off`/`save-all`/`save-on`. Old archives beyond
/// `keep_last` are rotated out, and the archive is uploaded to Google Drive
/// when authorized. Progress is reported through `tracing`, never stdout.
pub fn create_backup(server: &Server, config: &GlobalConfig, tmux: &TmuxClient) -> Result<PathBuf> {
    let controller = ServerController::new(tmux, config);
    let bcfg = server.config.backup.clone().unwrap_or_default();
    let dir = backup_dir(server);
    std::fs::create_dir_all(&dir)?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let out = dir.join(format!("{}-{}.zip", server.name, timestamp));
    let session = format!("anvil_{}", server.name);

    let was_online = controller.is_online(server);
    let mut stopped = false;
    if was_online {
        if bcfg.stop_server {
            tracing::info!(server = %server.name, "Stopping for a clean backup");
            let mut state = AppState::load(&config.state_path()).unwrap_or_default();
            let _ = controller.stop(server, &mut state);
            stopped = true;
        } else {
            tracing::info!(server = %server.name, "Flushing world (save-off/save-all)");
            let _ = tmux.send_keys(&session, "save-off");
            let _ = tmux.send_keys(&session, "save-all flush");
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    // Never archive the backups directory into the backup itself.
    let mut exclude = bcfg.exclude.clone();
    exclude.push("backups".to_string());
    exclude.push("backups/**".to_string());
    let archive_cfg = ArchiveConfig {
        include: bcfg.include.clone(),
        exclude,
    };

    let result = create_zip_archive(&server.path, &out, &archive_cfg);

    // Resume saving / restart regardless of how the archive went.
    if was_online {
        if stopped {
            let mut state = AppState::load(&config.state_path()).unwrap_or_default();
            if let Err(e) = controller.start(server, &mut state) {
                tracing::warn!(server = %server.name, error = %e, "Failed to restart after backup");
            }
        } else {
            let _ = tmux.send_keys(&session, "save-on");
        }
    }

    result.context("failed to create archive")?;
    rotate_backups(&dir, &server.name, bcfg.keep_last);
    upload_to_gdrive(config, &out);
    Ok(out)
}

/// Keep only the newest `keep_last` archives for a server, deleting older ones.
pub fn rotate_backups(dir: &Path, name: &str, keep_last: u32) {
    if keep_last == 0 {
        return;
    }
    let mut archives = list_archives(dir, name);
    archives.sort(); // timestamped names sort chronologically; newest last
    let keep = keep_last as usize;
    if archives.len() <= keep {
        return;
    }
    let remove_count = archives.len() - keep;
    for old in archives.into_iter().take(remove_count) {
        if std::fs::remove_file(&old).is_ok() {
            tracing::info!(file = %old.display(), "Rotated out old backup");
        }
    }
}

/// All archive paths `<name>-*.zip` in `dir`.
pub fn list_archives(dir: &Path, name: &str) -> Vec<PathBuf> {
    let prefix = format!("{}-", name);
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&prefix) && n.ends_with(".zip"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Upload a backup to Google Drive when a token exists (best-effort; logs only).
fn upload_to_gdrive(config: &GlobalConfig, file: &Path) {
    if !Path::new(&config.backup.token_path).exists() {
        return;
    }
    let token_path = config.backup.token_path.clone();
    let folder = config.backup.gdrive_folder.clone();
    let file = file.to_path_buf();
    let fname = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::warn!(error = %e, "Could not start uploader; kept local copy");
            return;
        }
    };
    let result = rt.block_on(async move {
        let oauth = oauth::OAuthClient::new(String::new(), String::new(), token_path);
        let token = oauth.get_valid_token().await?;
        let drive = gdrive::DriveClient::new(token);
        let folder_id = drive.find_or_create_folder(&folder, None).await?;
        drive
            .upload_file_resumable(&file, &fname, &folder_id, |_, _| {})
            .await?;
        anyhow::Ok(())
    });
    match result {
        Ok(_) => {
            tracing::info!(folder = %config.backup.gdrive_folder, "Uploaded backup to Google Drive")
        }
        Err(e) => tracing::warn!(error = %e, "Google Drive upload failed; kept local copy"),
    }
}
