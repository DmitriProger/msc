use crate::backup::archive::{create_zip_archive, ArchiveConfig};
use crate::backup::restore::extract_zip;
use crate::config::{GlobalConfig, VERSION};
use crate::error::AnvilError;
use crate::server::control::ServerController;
use crate::server::metrics::{format_bytes, format_uptime, get_process_uptime_secs, read_vmrss};
use crate::server::{discover_servers, find_server, Server};
use crate::state::AppState;
use crate::tmux::TmuxClient;
use crate::update::{self, UpdateOptions, UpdateOutcome};
use anyhow::Result;
use chrono::Utc;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

// Terminal-native palette: neutral text with a restrained gunmetal-teal accent.
const C_SUCCESS: Color = Color::Rgb {
    r: 0x5f,
    g: 0x7f,
    b: 0x7a,
};
const C_INFO: Color = Color::Rgb {
    r: 0xb8,
    g: 0xbe,
    b: 0xba,
};
const C_WARN: Color = Color::Rgb {
    r: 0xb8,
    g: 0xbe,
    b: 0xba,
};
const C_ERROR: Color = Color::Rgb {
    r: 0xb3,
    g: 0x7a,
    b: 0x72,
};
const C_DIM: Color = Color::Rgb {
    r: 0x74,
    g: 0x7b,
    b: 0x77,
};
const C_ACCENT: Color = Color::Rgb {
    r: 0x5f,
    g: 0x7f,
    b: 0x7a,
};
const C_BORDER: Color = Color::Rgb {
    r: 0x3f,
    g: 0x46,
    b: 0x43,
};

pub struct Printer {
    color: bool,
}

impl Printer {
    pub fn new() -> Self {
        Self {
            color: atty::is(atty::Stream::Stdout),
        }
    }

    fn print_line(&self, color: Color, label: Option<&str>, msg: &str) {
        if self.color {
            print!("{}", SetForegroundColor(color));
        }
        if let Some(label) = label {
            print!("{}: ", label);
        }
        println!("{}", msg);
        if self.color {
            print!("{}", ResetColor);
        }
        io::stdout().flush().ok();
    }

    fn print_error_line(&self, msg: &str) {
        if self.color {
            eprint!("{}", SetForegroundColor(C_ERROR));
        }
        eprintln!("error: {}", msg);
        if self.color {
            eprint!("{}", ResetColor);
        }
        io::stderr().flush().ok();
    }

    fn colorize(&self, color: Color, text: &str) -> String {
        if self.color {
            format!("{}{}{}", SetForegroundColor(color), text, ResetColor)
        } else {
            text.to_string()
        }
    }

    pub fn success(&self, msg: &str) {
        self.print_line(C_SUCCESS, None, msg);
    }

    pub fn info(&self, msg: &str) {
        self.print_line(C_INFO, None, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.print_line(C_WARN, Some("warning"), msg);
    }

    pub fn error(&self, msg: &str) {
        self.print_error_line(msg);
    }

    pub fn dim(&self, msg: &str) {
        self.print_line(C_DIM, None, msg);
    }

    pub fn blank(&self) {
        println!();
    }

    pub fn separator(&self) {
        if self.color {
            print!("{}", SetForegroundColor(C_BORDER));
        }
        println!("{}", "-".repeat(52));
        if self.color {
            print!("{}", ResetColor);
        }
        io::stdout().flush().ok();
    }

    pub fn block_start(&self, title: &str) {
        if !title.is_empty() {
            println!("{}", self.colorize(C_ACCENT, title));
            println!("{}", self.colorize(C_BORDER, &"-".repeat(title.len())));
        }
        io::stdout().flush().ok();
    }

    pub fn block_line(&self, msg: &str) {
        println!("{}", msg);
        io::stdout().flush().ok();
    }

    pub fn block_end(&self) {
        println!();
        io::stdout().flush().ok();
    }

    pub fn block_blank(&self) {
        self.block_line("");
    }

    #[allow(dead_code)]
    pub fn progress_bar(&self, value: f64, max: f64, width: usize) -> String {
        let ratio = if max > 0.0 {
            (value / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let filled = (ratio * width as f64).round() as usize;
        let empty = width.saturating_sub(filled);
        if self.color {
            format!(
                "{}{}{}{}{}",
                SetForegroundColor(C_SUCCESS),
                "#".repeat(filled),
                SetForegroundColor(C_BORDER),
                "-".repeat(empty),
                ResetColor
            )
        } else {
            format!("{}{}", "#".repeat(filled), "-".repeat(empty))
        }
    }
}

pub fn cmd_version(printer: &Printer) {
    let _ = printer;
    println!("anvil {}", VERSION);
}

pub fn cmd_list(global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);

    if servers.is_empty() {
        printer.warn(
            global_config
                .language
                .choose("No servers found", "Серверы не найдены"),
        );
        printer.dim(&format!(
            "{} {}",
            global_config.language.choose(
                "Create a server directory in",
                "Создай директорию сервера в"
            ),
            global_config.servers_root
        ));
        printer.dim(global_config.language.choose(
            "Each server needs a start.sh file",
            "В каждом сервере нужен start.sh",
        ));
        return Ok(());
    }

    let controller = ServerController::new(&tmux, global_config);

    // Sort: online first, then alphabetical
    let mut statuses: Vec<(&Server, bool)> = servers
        .iter()
        .map(|s| (s, controller.is_online(s)))
        .collect();
    statuses
        .sort_by(|(a, a_online), (b, b_online)| b_online.cmp(a_online).then(a.name.cmp(&b.name)));

    let col_name = 12usize;
    let col_status = 10usize;

    println!();
    if printer.color {
        println!(
            "{}  {:<width_name$}  {:<width_status$}  {:<12}  UPTIME{}",
            SetForegroundColor(C_DIM),
            "NAME",
            "STATUS",
            "RAM",
            ResetColor,
            width_name = col_name,
            width_status = col_status,
        );
    } else {
        println!(
            "  {:<width_name$}  {:<width_status$}  {:<12}  UPTIME",
            "NAME",
            "STATUS",
            "RAM",
            width_name = col_name,
            width_status = col_status
        );
    }
    printer.separator();

    for (server, online) in &statuses {
        let pid = controller.get_server_pid(server);
        let (ram_str, uptime_str) = if *online {
            if let Some(pid) = pid {
                let ram = read_vmrss(pid).unwrap_or(0);
                let ram_display = format!(
                    "{} / {}",
                    format_bytes(ram),
                    server.config.limits.memory_max
                );
                let uptime = get_process_uptime_secs(pid)
                    .map(format_uptime)
                    .unwrap_or_else(|| "-".to_string());
                (ram_display, uptime)
            } else {
                ("-".to_string(), "-".to_string())
            }
        } else {
            ("-".to_string(), "-".to_string())
        };

        let (status_color, status_str) = if *online {
            (
                C_SUCCESS,
                global_config.language.status_text(true).to_lowercase(),
            )
        } else {
            (
                C_ERROR,
                global_config.language.status_text(false).to_lowercase(),
            )
        };

        if printer.color {
            print!(
                "  {}{:<width_name$}{}  ",
                SetForegroundColor(Color::White),
                server.name,
                ResetColor,
                width_name = col_name,
            );
            print!(
                "{}{:<width_status$}{}  ",
                SetForegroundColor(status_color),
                status_str,
                ResetColor,
                width_status = col_status,
            );
            print!("{:<12}  ", ram_str);
            println!("{}{}{}", SetForegroundColor(C_DIM), uptime_str, ResetColor);
        } else {
            println!(
                "  {:<width_name$}  {:<width_status$}  {:<12}  {}",
                server.name,
                status_str,
                ram_str,
                uptime_str,
                width_name = col_name,
                width_status = col_status,
            );
        }
    }
    println!();
    Ok(())
}

pub fn cmd_update(
    global_config: &GlobalConfig,
    repo_override: Option<String>,
    version: Option<String>,
    check_only: bool,
    force: bool,
) -> Result<()> {
    let printer = Printer::new();
    let language = global_config.language;
    let repo = repo_override.unwrap_or_else(|| global_config.update.repo.clone());

    printer.info(language.choose("Checking GitHub releases...", "Проверяю релизы GitHub..."));
    if !check_only {
        printer.dim(language.choose(
            "Running Minecraft servers are not stopped during anvil update.",
            "Работающие Minecraft-серверы не останавливаются во время обновления anvil.",
        ));
    }

    let options = UpdateOptions {
        repo,
        version,
        check_only,
        force,
    };
    let rt = tokio::runtime::Runtime::new()?;

    match rt.block_on(update::run(options))? {
        UpdateOutcome::AlreadyCurrent { version } => {
            printer.success(&format!(
                "{} {}",
                language.choose("Anvil is already up to date:", "Anvil уже обновлен:"),
                version
            ));
        }
        UpdateOutcome::UpdateAvailable {
            current,
            latest,
            asset_name,
            asset_size,
        } => {
            printer.success(&format!(
                "{} {} -> {}",
                language.choose("Update available:", "Доступно обновление:"),
                current,
                latest
            ));
            printer.dim(&format!(
                "{} {} ({})",
                language.choose("Asset:", "Файл:"),
                asset_name,
                format_bytes(asset_size)
            ));
            printer.dim(language.choose(
                "Run `anvil update` to install it.",
                "Запусти `anvil update`, чтобы установить.",
            ));
        }
        UpdateOutcome::Updated {
            previous,
            current,
            path,
        } => {
            printer.success(&format!(
                "{} {} -> {}",
                language.choose("Anvil updated:", "Anvil обновлен:"),
                previous,
                current
            ));
            printer.dim(&format!(
                "{} {}",
                language.choose("Installed at", "Установлено в"),
                path.display()
            ));
            printer.dim(language.choose(
                "Existing Minecraft servers keep running. Restart anvil-watchdog only if you need the new daemon code now.",
                "Запущенные Minecraft-серверы продолжают работать. Перезапусти anvil-watchdog только если нужна новая логика демона прямо сейчас.",
            ));
        }
    }

    Ok(())
}

pub fn cmd_start(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let mut state = AppState::load(&global_config.state_path())?;
    let controller = ServerController::new(&tmux, global_config);

    if let Some(warning) = crate::server::control::xmx_warning(server) {
        printer.warn(&warning);
    }
    printer.info(&format!("Starting server {}...", name));
    match controller.start(server, &mut state) {
        Ok(pid) => {
            let pid_str = if pid > 0 {
                format!("pid {}", pid)
            } else {
                "starting".to_string()
            };
            printer.success(&format!(
                "Server {} is online  ·  {}  ·  tmux: anvil_{}",
                name, pid_str, name
            ));
        }
        Err(AnvilError::ServerAlreadyRunning(_)) => {
            printer.warn(&format!("Server {} is already running", name));
        }
        Err(e) => {
            printer.error(&format!("{}", e));
            return Err(e.into());
        }
    }
    Ok(())
}

pub fn cmd_stop(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let mut state = AppState::load(&global_config.state_path())?;
    let controller = ServerController::new(&tmux, global_config);

    printer.info(&format!("Sending stop command to {}...", name));
    printer.dim("Waiting for process to exit  (60s timeout, then SIGTERM/SIGKILL)");
    match controller.stop(server, &mut state) {
        Ok(()) => {
            printer.success(&format!("Server {} stopped", name));
        }
        Err(AnvilError::ServerNotRunning(_)) => {
            printer.warn(&format!("Server {} is already stopped", name));
        }
        Err(e) => {
            printer.error(&format!("{}", e));
            return Err(e.into());
        }
    }
    Ok(())
}

pub fn cmd_restart(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let mut state = AppState::load(&global_config.state_path())?;
    let controller = ServerController::new(&tmux, global_config);

    if let Some(warning) = crate::server::control::xmx_warning(server) {
        printer.warn(&warning);
    }
    printer.info(&format!("Restarting server {}...", name));
    match controller.restart(server, &mut state) {
        Ok(pid) => {
            let pid_str = if pid > 0 {
                format!("pid {}", pid)
            } else {
                "starting".to_string()
            };
            printer.success(&format!(
                "Server {} restarted  ·  {}  ·  tmux: anvil_{}",
                name, pid_str, name
            ));
        }
        Err(e) => {
            printer.error(&format!("{}", e));
            return Err(e.into());
        }
    }
    Ok(())
}

pub fn cmd_status(name: &str, global_config: &GlobalConfig) -> Result<()> {
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let controller = ServerController::new(&tmux, global_config);

    let online = controller.is_online(server);
    let pid = if online {
        controller.get_server_pid(server)
    } else {
        None
    };
    let status_str = if online { "online" } else { "offline" };

    println!("name:    {}", name);
    println!("status:  {}", status_str);
    if let Some(pid) = pid {
        println!("pid:     {}", pid);
        let ram = read_vmrss(pid).unwrap_or(0);
        println!("ram:     {} kB", ram / 1024);
        let uptime = get_process_uptime_secs(pid).unwrap_or(0);
        println!("uptime:  {}", uptime);
    } else {
        println!("pid:     -");
        println!("ram:     -");
        println!("uptime:  -");
    }
    Ok(())
}

pub fn cmd_send(name: &str, command: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let session = format!("anvil_{}", server.name);
    let controller = ServerController::new(&tmux, global_config);

    if !controller.is_online(server) {
        printer.error(&format!("Server {} is not running", name));
        return Err(AnvilError::ServerNotRunning(name.to_string()).into());
    }

    tmux.send_keys(&session, command)?;
    printer.success(&format!("Command sent to {}: {}", name, command));
    Ok(())
}

pub fn cmd_console(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let session = format!("anvil_{}", server.name);
    let controller = ServerController::new(&tmux, global_config);

    if !controller.is_online(server) {
        printer.error(&format!(
            "Server {} is not running - no tmux session found",
            name
        ));
        return Err(AnvilError::TmuxSessionNotFound(session).into());
    }

    tmux.attach_session(&session)?;
    Ok(())
}

fn backup_dir(server: &Server) -> PathBuf {
    server.path.join("backups")
}

/// Create a backup archive of a server. Hot-backs-up online servers by flushing
/// the world (`save-off` / `save-all`) unless `stop_server` is set, in which
/// case the server is stopped, archived, and restarted.
pub fn cmd_backup_create(name: &str, config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    let tmux = TmuxClient::new(&config.tmux_socket);
    let servers = discover_servers(config);
    let server = find_server(&servers, name)?;
    let controller = ServerController::new(&tmux, config);
    let bcfg = server.config.backup.clone().unwrap_or_default();

    let dir = backup_dir(server);
    std::fs::create_dir_all(&dir)?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let out = dir.join(format!("{}-{}.zip", name, timestamp));
    let session = format!("anvil_{}", name);

    let was_online = controller.is_online(server);
    let mut stopped = false;
    if was_online {
        if bcfg.stop_server {
            printer.info(&format!("Stopping {} for a clean backup...", name));
            let mut state = AppState::load(&config.state_path()).unwrap_or_default();
            let _ = controller.stop(server, &mut state);
            stopped = true;
        } else {
            printer.info("Flushing world to disk (save-off / save-all)...");
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

    printer.info(&format!("Archiving {} -> {}", name, out.display()));
    let result = create_zip_archive(&server.path, &out, &archive_cfg);

    // Resume saving / restart regardless of how the archive went.
    if was_online {
        if stopped {
            let mut state = AppState::load(&config.state_path()).unwrap_or_default();
            if let Err(e) = controller.start(server, &mut state) {
                printer.warn(&format!("Failed to restart {} after backup: {}", name, e));
            }
        } else {
            let _ = tmux.send_keys(&session, "save-on");
        }
    }

    result.map_err(|e| AnvilError::Backup(e.to_string()))?;

    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    printer.success(&format!(
        "Backup created: {} ({})",
        out.display(),
        format_bytes(size)
    ));

    rotate_backups(&dir, name, bcfg.keep_last, &printer);
    maybe_upload_gdrive(config, &out, &printer);
    Ok(())
}

/// Keep only the newest `keep_last` archives for a server, deleting older ones.
fn rotate_backups(dir: &Path, name: &str, keep_last: u32, printer: &Printer) {
    if keep_last == 0 {
        return;
    }
    let mut archives: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&format!("{}-", name)) && n.ends_with(".zip"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return,
    };
    // Timestamped names sort chronologically; newest last.
    archives.sort();
    let keep = keep_last as usize;
    if archives.len() <= keep {
        return;
    }
    let remove_count = archives.len() - keep;
    for old in archives.into_iter().take(remove_count) {
        if std::fs::remove_file(&old).is_ok() {
            printer.dim(&format!("Rotated out old backup: {}", old.display()));
        }
    }
}

/// Upload a backup to Google Drive when authorized; otherwise note local-only.
fn maybe_upload_gdrive(config: &GlobalConfig, file: &Path, printer: &Printer) {
    if !Path::new(&config.backup.token_path).exists() {
        printer.dim("Stored locally. Run `anvil backup auth` to enable Google Drive uploads.");
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
            printer.warn(&format!(
                "Could not start uploader (kept local copy): {}",
                e
            ));
            return;
        }
    };
    let result = rt.block_on(async move {
        let oauth =
            crate::backup::oauth::OAuthClient::new(String::new(), String::new(), token_path);
        let token = oauth.get_valid_token().await?;
        let drive = crate::backup::gdrive::DriveClient::new(token);
        let folder_id = drive.find_or_create_folder(&folder, None).await?;
        drive
            .upload_file_resumable(&file, &fname, &folder_id, |_, _| {})
            .await?;
        anyhow::Ok(())
    });
    match result {
        Ok(_) => printer.success(&format!(
            "Uploaded to Google Drive folder '{}'",
            config.backup.gdrive_folder
        )),
        Err(e) => printer.warn(&format!(
            "Google Drive upload failed (local copy kept): {}",
            e
        )),
    }
}

pub fn cmd_backup_list(name: &str, config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    let servers = discover_servers(config);
    let server = find_server(&servers, name)?;
    let dir = backup_dir(server);

    let mut archives: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&format!("{}-", name)) && n.ends_with(".zip"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    if archives.is_empty() {
        printer.info(&format!(
            "No backups found for {} in {}",
            name,
            dir.display()
        ));
        return Ok(());
    }
    archives.sort();
    archives.reverse(); // newest first
    println!();
    for archive in &archives {
        let size = std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
        let fname = archive
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        println!("  {:<40}  {}", fname, format_bytes(size));
    }
    println!();
    Ok(())
}

pub fn cmd_backup_restore(name: &str, file: &str, config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    let tmux = TmuxClient::new(&config.tmux_socket);
    let servers = discover_servers(config);
    let server = find_server(&servers, name)?;
    let controller = ServerController::new(&tmux, config);

    // Accept either a bare filename (resolved in the backups dir) or a path.
    let candidate = PathBuf::from(file);
    let archive = if candidate.is_absolute() || candidate.exists() {
        candidate
    } else {
        backup_dir(server).join(file)
    };
    if !archive.exists() {
        printer.error(&format!("Backup file not found: {}", archive.display()));
        return Err(AnvilError::Backup(format!("not found: {}", archive.display())).into());
    }

    if controller.is_online(server) {
        printer.info(&format!("Stopping {} before restore...", name));
        let mut state = AppState::load(&config.state_path()).unwrap_or_default();
        let _ = controller.stop(server, &mut state);
    }

    printer.info(&format!("Restoring {} from {}...", name, archive.display()));
    extract_zip(&archive, &server.path).map_err(|e| AnvilError::Backup(e.to_string()))?;
    printer.success(&format!(
        "Restored {}. Start it with `anvil {} start`.",
        name, name
    ));
    Ok(())
}

pub fn cmd_install(global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    install::run_install(global_config, &printer)
}

pub fn cmd_uninstall(_global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    printer.info("Removing anvil-watchdog systemd unit...");
    run_shell("systemctl", &["disable", "--now", "anvil-watchdog"]);
    run_shell("rm", &["-f", "/etc/systemd/system/anvil-watchdog.service"]);
    run_shell("systemctl", &["daemon-reload"]);
    printer.success("anvil-watchdog removed");
    Ok(())
}

fn run_shell(cmd: &str, args: &[&str]) {
    if let Err(e) = std::process::Command::new(cmd).args(args).status() {
        tracing::warn!(cmd = %cmd, error = %e, "Command failed");
    }
}

pub fn print_server_not_found(name: &str, servers: &[Server]) {
    let printer = Printer::new();
    printer.error(&format!("Server \"{}\" not found", name));
    if !servers.is_empty() {
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        printer.dim(&format!("Available servers: {}", names.join(", ")));
    }
    printer.dim("Run `anvil list` to see all servers");
}

mod install {
    use super::*;
    use std::process::Command;

    const SYSTEMD_UNIT: &str = r#"[Unit]
Description=Anvil Watchdog - Anvil daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/anvil --watchdog
Restart=always
RestartSec=5
User=minecraft
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#;

    const LOGROTATE_CONF: &str = r#"/var/log/anvil/anvil.log {
    weekly
    rotate 4
    compress
    missingok
    notifempty
}
"#;

    pub fn run_install(_config: &GlobalConfig, printer: &Printer) -> Result<()> {
        let mut items: Vec<(bool, &str)> = Vec::new();

        printer.blank();
        printer.block_start("Anvil Install");
        printer.block_blank();

        // 1. User
        let user_exists = user_exists("minecraft");
        let generated_password = if !user_exists {
            let password = generate_password(24);
            let status = Command::new("useradd")
                .args([
                    "--system",
                    "--shell",
                    "/bin/bash",
                    "--home-dir",
                    "/opt/minecraft",
                    "--create-home",
                    "--comment",
                    "Minecraft Server Manager",
                    "minecraft",
                ])
                .status();
            match status {
                Ok(s) if s.success() => {
                    let chpasswd_input = format!("minecraft:{}", password);
                    let mut child = Command::new("chpasswd")
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .ok();
                    if let Some(ref mut c) = child {
                        if let Some(ref mut stdin) = c.stdin {
                            use std::io::Write;
                            let _ = stdin.write_all(chpasswd_input.as_bytes());
                        }
                        let _ = c.wait();
                    }
                    items.push((true, "User minecraft created"));
                    Some(password)
                }
                _ => {
                    items.push((false, "Failed to create user minecraft"));
                    None
                }
            }
        } else {
            items.push((true, "User minecraft already exists (skipped)"));
            None
        };

        // 2. Directories
        for dir in &[
            "/opt/minecraft",
            "/var/lib/anvil",
            "/var/log/anvil",
            "/etc/anvil",
        ] {
            match std::fs::create_dir_all(dir) {
                Ok(_) => items.push((true, dir_label(dir))),
                Err(e) => {
                    tracing::warn!(dir = dir, error = %e, "Failed to create directory");
                    items.push((false, dir_label(dir)));
                }
            }
        }

        // 3. Permissions
        let perm_ok = Command::new("chown")
            .args([
                "-R",
                "minecraft:minecraft",
                "/opt/minecraft",
                "/var/lib/anvil",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        items.push((perm_ok, "Permissions set"));

        // 4. Config
        if !std::path::Path::new("/etc/anvil/config.toml").exists() {
            match GlobalConfig::save_default("/etc/anvil/config.toml") {
                Ok(_) => items.push((true, "/etc/anvil/config.toml written")),
                Err(_) => items.push((false, "/etc/anvil/config.toml write failed")),
            }
        } else {
            items.push((true, "/etc/anvil/config.toml already exists (skipped)"));
        }

        // 5. state.json
        let state_path = std::path::Path::new("/var/lib/anvil/state.json");
        if !state_path.exists() {
            let state = crate::state::AppState::default();
            match state.save(state_path) {
                Ok(_) => items.push((true, "state.json created")),
                Err(_) => items.push((false, "state.json creation failed")),
            }
        }

        // 6. logrotate
        if let Err(e) = std::fs::write("/etc/logrotate.d/anvil", LOGROTATE_CONF) {
            tracing::warn!(error = %e, "Failed to write logrotate config");
        }

        // 7. systemd unit
        match std::fs::write("/etc/systemd/system/anvil-watchdog.service", SYSTEMD_UNIT) {
            Ok(_) => items.push((true, "systemd unit written")),
            Err(_) => items.push((false, "systemd unit write failed")),
        }

        // 8. Enable and start
        let daemon_ok = Command::new("systemctl")
            .arg("daemon-reload")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let enable_ok = Command::new("systemctl")
            .args(["enable", "--now", "anvil-watchdog"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        items.push((
            (daemon_ok && enable_ok),
            "anvil-watchdog enabled and started",
        ));

        // Print items
        for (ok, label) in &items {
            let line = if *ok {
                format!("ok    {}", label)
            } else {
                format!("fail  {}", label)
            };
            printer.block_line(&line);
        }

        printer.block_blank();
        printer.block_line(&format!("Anvil v{} installed successfully", VERSION));
        printer.block_line("Run `anvil` to open the control panel");
        printer.block_blank();
        printer.block_end();

        // Show generated password if new user was created
        if let Some(password) = generated_password {
            printer.block_start("New User Created");
            printer.block_blank();
            printer.block_line("User      minecraft");
            printer.block_line(&format!("Password  {}", password));
            printer.block_blank();
            printer.block_line("Warning: this password is shown only once. Save it now.");
            printer.block_blank();
            printer.block_end();

            printer.block_start("SSH Connection");
            printer.block_blank();
            printer.block_line("ssh minecraft@<your-server-ip>");
            printer.block_blank();
            printer.block_line("Replace <your-server-ip> with your actual IP");
            printer.block_line("To find your IP: hostname -I | awk '{print $1}'");
            printer.block_line("You will be prompted for the password above");
            printer.block_blank();
            printer.block_end();
        }

        Ok(())
    }

    fn user_exists(username: &str) -> bool {
        Command::new("id")
            .arg(username)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn dir_label(dir: &str) -> &'static str {
        match dir {
            "/opt/minecraft" => "/opt/minecraft created",
            "/var/lib/anvil" => "/var/lib/anvil created",
            "/var/log/anvil" => "/var/log/anvil created",
            "/etc/anvil" => "/etc/anvil created",
            _ => "directory created",
        }
    }

    fn generate_password(len: usize) -> String {
        use std::fs::File;
        use std::io::Read;
        let charset: Vec<char> =
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*"
                .chars()
                .collect();
        let mut rng_bytes = vec![0u8; len];
        if let Ok(mut f) = File::open("/dev/urandom") {
            let _ = f.read_exact(&mut rng_bytes);
        }
        rng_bytes
            .iter()
            .map(|&b| charset[b as usize % charset.len()])
            .collect()
    }
}
