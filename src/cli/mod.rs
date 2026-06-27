use crate::config::{GlobalConfig, VERSION};
use crate::error::AnvilError;
use crate::server::control::ServerController;
use crate::server::metrics::{format_bytes, format_uptime, get_process_uptime_secs, read_vmrss};
use crate::server::{discover_servers, find_server, Server};
use crate::state::AppState;
use crate::screen::ScreenClient;
use crate::update::{self, UpdateOptions, UpdateOutcome};
use anyhow::Result;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
    r: 0xd4,
    g: 0xb2,
    b: 0x7c,
};
const C_ERROR: Color = Color::Rgb {
    r: 0xc8,
    g: 0x7a,
    b: 0x7a,
};
const C_DIM: Color = Color::Rgb {
    r: 0x7c,
    g: 0x82,
    b: 0x7e,
};

pub struct Printer {
    color: bool,
}

impl Printer {
    pub fn new() -> Self {
        Self { color: true }
    }

    pub fn print(&self, color: Color, prefix: &str, msg: &str) {
        if self.color {
            println!(
                "{}  {:<8}{}  {}",
                SetForegroundColor(color),
                prefix,
                ResetColor,
                msg
            );
        } else {
            println!("  {:<8}  {}", prefix, msg);
        }
    }

    pub fn success(&self, msg: &str) {
        self.print(C_SUCCESS, "success", msg);
    }

    pub fn info(&self, msg: &str) {
        self.print(C_INFO, "info", msg);
    }

    pub fn warn(&self, msg: &str) {
        self.print(C_WARN, "warning", msg);
    }

    pub fn error(&self, msg: &str) {
        self.print(C_ERROR, "error", msg);
    }

    pub fn dim(&self, msg: &str) {
        if self.color {
            println!("{}{}{}", SetForegroundColor(C_DIM), msg, ResetColor);
        } else {
            println!("{}", msg);
        }
    }

    pub fn separator(&self) {
        if self.color {
            println!(
                "{}-------------------------------------------------{}",
                SetForegroundColor(C_DIM),
                ResetColor
            );
        } else {
            println!("-------------------------------------------------");
        }
    }
}

pub fn cmd_version(printer: &Printer) {
    printer.success(&format!("Anvil v{}", VERSION));
}

pub fn cmd_list(global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);

    if servers.is_empty() {
        printer.info(
            global_config
                .language
                .choose("No servers found", "Серверы не найдены"),
        );
        return Ok(());
    }

    let controller = ServerController::new(&tmux, global_config);

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
        }
    }

    Ok(())
}

pub fn cmd_start(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let mut state = AppState::load(&global_config.state_path())?;
    let controller = ServerController::new(&tmux, global_config);

    printer.info(&format!("Starting server {}...", name));
    match controller.start(server, &mut state) {
        Ok(pid) => {
            let pid_str = if pid > 0 {
                format!("pid {}", pid)
            } else {
                "starting".to_string()
            };
            printer.success(&format!(
                "Server {} is online  ·  {}  ·  screen: {}",
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
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let mut state = AppState::load(&global_config.state_path())?;
    let controller = ServerController::new(&tmux, global_config);

    printer.info(&format!("Sending stop command to {}...", name));
    printer.dim("Waiting for process to exit");
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
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let mut state = AppState::load(&global_config.state_path())?;
    let controller = ServerController::new(&tmux, global_config);

    printer.info(&format!("Restarting server {}...", name));
    match controller.restart(server, &mut state) {
        Ok(pid) => {
            let pid_str = if pid > 0 {
                format!("pid {}", pid)
            } else {
                "starting".to_string()
            };
            printer.success(&format!(
                "Server {} restarted  ·  {}  ·  screen: {}",
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
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
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
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let controller = ServerController::new(&tmux, global_config);

    if !controller.is_online(server) {
        printer.error(&format!("Server {} is not running", name));
        return Err(AnvilError::ServerNotRunning(name.to_string()).into());
    }

    tmux.send_keys(&server.name, command)?;
    printer.success(&format!("Command sent to {}: {}", name, command));
    Ok(())
}

pub fn cmd_console(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    ScreenClient::check_installed()?;
    let tmux = ScreenClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let controller = ServerController::new(&tmux, global_config);

    if !controller.is_online(server) {
        printer.error(&format!(
            "Server {} is not running - no screen session found",
            name
        ));
        return Err(AnvilError::TmuxSessionNotFound(server.name.clone()).into());
    }

    println!("\n====================================================");
    println!("Connecting to Minecraft Console.");
    println!("- To DETACH and return: Press Ctrl+A, then D");
    println!("- If the input field disappears (on scroll): Press Q to return it");
    println!("====================================================\n");
    std::thread::sleep(std::time::Duration::from_millis(1500));

    tmux.attach_session(&server.name)?;
    Ok(())
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
