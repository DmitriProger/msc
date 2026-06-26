use crate::config::{GlobalConfig, VERSION};
use crate::error::MscError;
use crate::server::control::ServerController;
use crate::server::metrics::{format_bytes, format_uptime, get_process_uptime_secs, read_vmrss};
use crate::server::{discover_servers, find_server, Server};
use crate::state::AppState;
use crate::tmux::TmuxClient;
use anyhow::Result;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use std::io::{self, Write};

// ─── Color palette ────────────────────────────────────────────────────────────
const C_SUCCESS: Color = Color::Rgb {
    r: 0x87,
    g: 0xc9,
    b: 0x8e,
};
const C_INFO: Color = Color::Rgb {
    r: 0x7f,
    g: 0xbf,
    b: 0xff,
};
const C_WARN: Color = Color::Rgb {
    r: 0xd4,
    g: 0xa9,
    b: 0x6a,
};
const C_ERROR: Color = Color::Rgb {
    r: 0xc9,
    g: 0x70,
    b: 0x70,
};
const C_DIM: Color = Color::Rgb {
    r: 0x80,
    g: 0x80,
    b: 0x80,
};
const C_ACCENT: Color = Color::Rgb {
    r: 0x7f,
    g: 0xbf,
    b: 0xff,
};
const C_BORDER: Color = Color::Rgb {
    r: 0x3a,
    g: 0x3a,
    b: 0x3a,
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

    fn print_colored(&self, color: Color, prefix: &str, msg: &str) {
        if self.color {
            print!("{}{}{}", SetForegroundColor(color), prefix, ResetColor);
        } else {
            print!("{}", prefix);
        }
        println!(" {}", msg);
        io::stdout().flush().ok();
    }

    pub fn success(&self, msg: &str) {
        self.print_colored(C_SUCCESS, "  ✓", msg);
    }

    pub fn info(&self, msg: &str) {
        self.print_colored(C_INFO, "  →", msg);
    }

    pub fn warn(&self, msg: &str) {
        self.print_colored(C_WARN, "  ⚠", msg);
    }

    pub fn error(&self, msg: &str) {
        self.print_colored(C_ERROR, "  ✗", msg);
    }

    pub fn dim(&self, msg: &str) {
        self.print_colored(C_DIM, "  ·", msg);
    }

    pub fn blank(&self) {
        println!();
    }

    pub fn separator(&self) {
        if self.color {
            print!("{}", SetForegroundColor(C_BORDER));
        }
        println!("  {}", "─".repeat(52));
        if self.color {
            print!("{}", ResetColor);
        }
        io::stdout().flush().ok();
    }

    pub fn block_start(&self, title: &str) {
        let width: usize = 57;
        let title_part = format!("─  {}  ", title);
        let remaining = width.saturating_sub(title_part.len() + 2);
        let border = format!("╭{}{}╮", title_part, "─".repeat(remaining));
        if self.color {
            println!("{}{}{}", SetForegroundColor(C_ACCENT), border, ResetColor);
            println!(
                "{}│{}│{}",
                SetForegroundColor(C_BORDER),
                " ".repeat(width),
                ResetColor
            );
        } else {
            println!("{}", border);
            println!("│{}│", " ".repeat(width));
        }
        io::stdout().flush().ok();
    }

    pub fn block_line(&self, msg: &str) {
        let width: usize = 57;
        let padded = format!("  {}  ", msg);
        let padding = width.saturating_sub(padded.chars().count());
        if self.color {
            println!(
                "{}│{}{}{} │{}",
                SetForegroundColor(C_BORDER),
                ResetColor,
                padded,
                " ".repeat(padding),
                ResetColor
            );
        } else {
            println!("│{}{}│", padded, " ".repeat(padding));
        }
        io::stdout().flush().ok();
    }

    pub fn block_end(&self) {
        let width = 57;
        let border = format!("╰{}╯", "─".repeat(width));
        if self.color {
            println!("{}{}{}", SetForegroundColor(C_BORDER), border, ResetColor);
        } else {
            println!("{}", border);
        }
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
        let bar_color = if ratio < 0.60 {
            C_SUCCESS
        } else if ratio < 0.85 {
            C_WARN
        } else {
            C_ERROR
        };
        if self.color {
            format!(
                "{}{}{}{}{}",
                SetForegroundColor(bar_color),
                "█".repeat(filled),
                SetForegroundColor(C_BORDER),
                "░".repeat(empty),
                ResetColor
            )
        } else {
            format!("{}{}", "█".repeat(filled), "░".repeat(empty))
        }
    }
}

// ─── Commands ────────────────────────────────────────────────────────────────

pub fn cmd_version(printer: &Printer) {
    printer.info(&format!("msc version {}", VERSION));
}

pub fn cmd_list(global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);

    if servers.is_empty() {
        printer.warn("No servers found");
        printer.dim(&format!(
            "Create a server directory in {}",
            global_config.servers_root
        ));
        printer.dim("Each server needs a start.sh file");
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
                    .unwrap_or_else(|| "—".to_string());
                (ram_display, uptime)
            } else {
                ("—".to_string(), "—".to_string())
            }
        } else {
            ("—".to_string(), "—".to_string())
        };

        let (status_color, status_str) = if *online {
            (C_SUCCESS, "online")
        } else {
            (C_ERROR, "offline")
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

pub fn cmd_start(name: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
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
                "Server {} is online  ·  {}  ·  tmux: msc_{}",
                name, pid_str, name
            ));
        }
        Err(MscError::ServerAlreadyRunning(_)) => {
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
    printer.dim("Waiting for process to exit  (30s timeout)");
    match controller.stop(server, &mut state) {
        Ok(()) => {
            printer.success(&format!("Server {} stopped", name));
        }
        Err(MscError::ServerNotRunning(_)) => {
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

    printer.info(&format!("Restarting server {}...", name));
    match controller.restart(server, &mut state) {
        Ok(pid) => {
            let pid_str = if pid > 0 {
                format!("pid {}", pid)
            } else {
                "starting".to_string()
            };
            printer.success(&format!(
                "Server {} restarted  ·  {}  ·  tmux: msc_{}",
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
    let pid = controller.get_server_pid(server);
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
        println!("pid:     —");
        println!("ram:     —");
        println!("uptime:  —");
    }
    Ok(())
}

pub fn cmd_send(name: &str, command: &str, global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    TmuxClient::check_installed()?;
    let tmux = TmuxClient::new(&global_config.tmux_socket);
    let servers = discover_servers(global_config);
    let server = find_server(&servers, name)?;
    let session = format!("msc_{}", server.name);

    if !tmux.session_exists(&session) {
        printer.error(&format!("Server {} is not running", name));
        return Err(MscError::ServerNotRunning(name.to_string()).into());
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
    let session = format!("msc_{}", server.name);

    if !tmux.session_exists(&session) {
        printer.error(&format!(
            "Server {} is not running — no tmux session found",
            name
        ));
        return Err(MscError::TmuxSessionNotFound(session).into());
    }

    tmux.attach_session(&session)?;
    Ok(())
}

pub fn cmd_install(global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    install::run_install(global_config, &printer)
}

pub fn cmd_uninstall(_global_config: &GlobalConfig) -> Result<()> {
    let printer = Printer::new();
    printer.info("Removing msc-watchdog systemd unit...");
    run_shell("systemctl", &["disable", "--now", "msc-watchdog"]);
    run_shell("rm", &["-f", "/etc/systemd/system/msc-watchdog.service"]);
    run_shell("systemctl", &["daemon-reload"]);
    printer.success("msc-watchdog removed");
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
    printer.dim("Run `msc list` to see all servers");
}

mod install {
    use super::*;
    use std::process::Command;

    const SYSTEMD_UNIT: &str = r#"[Unit]
Description=MSC Watchdog — Minecraft Server Control Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/msc --watchdog
Restart=always
RestartSec=5
User=minecraft
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#;

    const LOGROTATE_CONF: &str = r#"/var/log/msc/msc.log {
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
        printer.block_start("MSC Install");
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
        for dir in &["/opt/minecraft", "/var/lib/msc", "/var/log/msc", "/etc/msc"] {
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
                "/var/lib/msc",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        items.push((perm_ok, "Permissions set"));

        // 4. Config
        if !std::path::Path::new("/etc/msc/config.toml").exists() {
            match GlobalConfig::save_default("/etc/msc/config.toml") {
                Ok(_) => items.push((true, "/etc/msc/config.toml written")),
                Err(_) => items.push((false, "/etc/msc/config.toml write failed")),
            }
        } else {
            items.push((true, "/etc/msc/config.toml already exists (skipped)"));
        }

        // 5. state.json
        let state_path = std::path::Path::new("/var/lib/msc/state.json");
        if !state_path.exists() {
            let state = crate::state::AppState::default();
            match state.save(state_path) {
                Ok(_) => items.push((true, "state.json created")),
                Err(_) => items.push((false, "state.json creation failed")),
            }
        }

        // 6. logrotate
        if let Err(e) = std::fs::write("/etc/logrotate.d/msc", LOGROTATE_CONF) {
            tracing::warn!(error = %e, "Failed to write logrotate config");
        }

        // 7. systemd unit
        match std::fs::write("/etc/systemd/system/msc-watchdog.service", SYSTEMD_UNIT) {
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
            .args(["enable", "--now", "msc-watchdog"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        items.push(((daemon_ok && enable_ok), "msc-watchdog enabled and started"));

        // Print items
        for (ok, label) in &items {
            let line = if *ok {
                format!("  ✓  {}", label)
            } else {
                format!("  ✗  {}", label)
            };
            printer.block_line(&line);
        }

        printer.block_blank();
        printer.block_line(&format!("  ·  MSC v{} installed successfully", VERSION));
        printer.block_line("  ·  Run `msc` to open the control panel");
        printer.block_blank();
        printer.block_end();

        // Show generated password if new user was created
        if let Some(password) = generated_password {
            printer.block_start("New User Created");
            printer.block_blank();
            printer.block_line("  User      minecraft");
            printer.block_line(&format!("  Password  {}", password));
            printer.block_blank();
            printer.block_line("  ⚠  This password is shown only once. Save it now.");
            printer.block_blank();
            printer.block_end();

            printer.block_start("SSH Connection");
            printer.block_blank();
            printer.block_line("  ssh minecraft@<your-server-ip>");
            printer.block_blank();
            printer.block_line("  ·  Replace <your-server-ip> with your actual IP");
            printer.block_line("  ·  To find your IP: hostname -I | awk '{print $1}'");
            printer.block_line("  ·  You will be prompted for the password above");
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
            "/var/lib/msc" => "/var/lib/msc created",
            "/var/log/msc" => "/var/log/msc created",
            "/etc/msc" => "/etc/msc created",
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
