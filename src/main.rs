mod backup;
mod cli;
mod config;
mod error;
mod server;
mod state;
mod tmux;
mod tui;
mod update;
mod watchdog;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use config::GlobalConfig;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Debug, Parser)]
#[command(
    name = "anvil",
    about = "Anvil - CLI for managing Minecraft servers on Linux",
    version = config::VERSION,
    disable_help_subcommand = true,
)]
struct Cli {
    /// Run as watchdog daemon (internal, used by systemd)
    #[arg(long, hide = true)]
    watchdog: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List all servers and their status
    List,

    /// Install systemd watchdog unit
    Install,

    /// Remove systemd watchdog unit
    Uninstall,

    /// Show version
    Version,

    /// Update anvil from a GitHub release
    Update(UpdateArgs),

    /// Backup management
    Backup {
        #[command(subcommand)]
        action: BackupCommand,
    },

    /// Server-specific commands
    #[command(external_subcommand)]
    Server(Vec<String>),
}

#[derive(Debug, Args)]
struct UpdateArgs {
    /// Only check if an update is available
    #[arg(long)]
    check: bool,

    /// Install even when the latest release matches the current version
    #[arg(long)]
    force: bool,

    /// Override update repository in owner/name format
    #[arg(long)]
    repo: Option<String>,

    /// Install a specific release tag, for example v1.1.0
    #[arg(long)]
    version: Option<String>,
}

#[derive(Debug, Subcommand)]
enum BackupCommand {
    /// Authorize with Google Drive (one-time setup)
    Auth,
    /// Show Google Drive connection status
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let global_config = GlobalConfig::load()?;

    init_tracing(&global_config, cli.watchdog);

    if cli.watchdog {
        watchdog::run_watchdog(global_config)?;
        return Ok(());
    }

    match cli.command {
        None => {
            tmux::TmuxClient::check_installed().map_err(|e| {
                eprintln!("{}", e);
                e
            })?;
            tui::run_main_tui(global_config)?;
        }
        Some(Command::List) => {
            cli::cmd_list(&global_config)?;
        }
        Some(Command::Install) => {
            cli::cmd_install(&global_config)?;
        }
        Some(Command::Uninstall) => {
            cli::cmd_uninstall(&global_config)?;
        }
        Some(Command::Version) => {
            let printer = cli::Printer::new();
            cli::cmd_version(&printer);
        }
        Some(Command::Update(args)) => {
            cli::cmd_update(
                &global_config,
                args.repo,
                args.version,
                args.check,
                args.force,
            )?;
        }
        Some(Command::Backup { action }) => {
            handle_backup(action, &global_config)?;
        }
        Some(Command::Server(args)) => {
            handle_server_args(args, &global_config)?;
        }
    }

    Ok(())
}

fn handle_server_args(args: Vec<String>, global_config: &GlobalConfig) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: anvil <name> [start|stop|restart|console|status|send <cmd>|backup]");
        std::process::exit(1);
    }

    let name = &args[0];
    let sub = args.get(1).map(|s| s.as_str());

    let servers = server::discover_servers(global_config);

    match sub {
        None => {
            if server::find_server(&servers, name).is_err() {
                cli::print_server_not_found(name, &servers);
                std::process::exit(1);
            }
            tmux::TmuxClient::check_installed().map_err(|e| {
                eprintln!("{}", e);
                e
            })?;
            tui::run_server_tui(name.clone(), global_config.clone())?;
        }
        Some("start") => {
            check_server_exists(&servers, name);
            let _ = cli::cmd_start(name, global_config).map_err(|_| std::process::exit(1));
        }
        Some("stop") => {
            check_server_exists(&servers, name);
            let _ = cli::cmd_stop(name, global_config).map_err(|_| std::process::exit(1));
        }
        Some("restart") => {
            check_server_exists(&servers, name);
            let _ = cli::cmd_restart(name, global_config).map_err(|_| std::process::exit(1));
        }
        Some("console") => {
            check_server_exists(&servers, name);
            let _ = cli::cmd_console(name, global_config).map_err(|_| std::process::exit(1));
        }
        Some("status") => {
            check_server_exists(&servers, name);
            let _ = cli::cmd_status(name, global_config).map_err(|_| std::process::exit(1));
        }
        Some("send") => {
            check_server_exists(&servers, name);
            let cmd = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if cmd.is_empty() {
                eprintln!("Usage: anvil {} send \"<command>\"", name);
                std::process::exit(1);
            }
            let _ = cli::cmd_send(name, cmd, global_config).map_err(|_| std::process::exit(1));
        }
        Some("backup") => {
            check_server_exists(&servers, name);
            let backup_sub = args.get(2).map(|s| s.as_str());
            handle_server_backup(
                name,
                backup_sub,
                args.get(3).map(|s| s.as_str()),
                global_config,
            )?;
        }
        Some(unknown) => {
            eprintln!("Unknown subcommand: {}", unknown);
            eprintln!(
                "Usage: anvil {} [start|stop|restart|console|status|send|backup]",
                name
            );
            std::process::exit(1);
        }
    }
    Ok(())
}

fn check_server_exists(servers: &[server::Server], name: &str) {
    if server::find_server(servers, name).is_err() {
        cli::print_server_not_found(name, servers);
        std::process::exit(1);
    }
}

fn handle_backup(action: BackupCommand, config: &GlobalConfig) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let printer = cli::Printer::new();
    match action {
        BackupCommand::Auth => {
            let client_id = std::env::var("GDRIVE_CLIENT_ID").unwrap_or_default();
            let client_secret = std::env::var("GDRIVE_CLIENT_SECRET").unwrap_or_default();
            if client_id.is_empty() || client_secret.is_empty() {
                printer.error("Set GDRIVE_CLIENT_ID and GDRIVE_CLIENT_SECRET before running auth");
                std::process::exit(1);
            }
            let oauth = backup::oauth::OAuthClient::new(
                client_id,
                client_secret,
                config.backup.token_path.clone(),
            );
            printer.block_start("Google Drive Auth");
            printer.block_blank();
            printer.block_line("Opening authorization flow...");
            printer.block_blank();
            rt.block_on(oauth.authorize())?;
            printer.block_blank();
            printer.block_line("Authorized successfully");
            printer.block_line(&format!("Token saved to {}", config.backup.token_path));
            printer.block_blank();
            printer.block_line("Token will refresh automatically");
            printer.block_line("Run `anvil backup status` to verify connection");
            printer.block_blank();
            printer.block_end();
        }
        BackupCommand::Status => {
            let oauth = backup::oauth::OAuthClient::new(
                String::new(),
                String::new(),
                config.backup.token_path.clone(),
            );
            if oauth.is_authorized() {
                printer.success("Authorized with Google Drive");
                printer.dim(&format!("Token: {}", config.backup.token_path));
            } else {
                printer.error("Not authorized. Run `anvil backup auth`");
            }
        }
    }
    Ok(())
}

fn handle_server_backup(
    name: &str,
    sub: Option<&str>,
    arg: Option<&str>,
    config: &GlobalConfig,
) -> Result<()> {
    let printer = cli::Printer::new();
    match sub {
        None => {
            let _ = cli::cmd_backup_create(name, config).map_err(|_| std::process::exit(1));
        }
        Some("list") => {
            let _ = cli::cmd_backup_list(name, config).map_err(|_| std::process::exit(1));
        }
        Some("restore") => {
            let filename = arg.unwrap_or("");
            if filename.is_empty() {
                printer.error("Usage: anvil <name> backup restore <filename>");
                std::process::exit(1);
            }
            printer.warn(&format!(
                "This will overwrite the current server data for {}",
                name
            ));
            printer.warn("The server will be stopped during restore");
            print!("\nProceed? [y/N] ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            if input.trim().to_lowercase() != "y" {
                printer.info("Cancelled");
                return Ok(());
            }
            let _ =
                cli::cmd_backup_restore(name, filename, config).map_err(|_| std::process::exit(1));
        }
        Some(unknown) => {
            printer.error(&format!("Unknown backup subcommand: {}", unknown));
            std::process::exit(1);
        }
    }
    Ok(())
}

fn init_tracing(config: &GlobalConfig, watchdog: bool) {
    if !watchdog && std::env::var_os("ANVIL_LOG").is_none() {
        return;
    }

    let filter = EnvFilter::try_from_env("ANVIL_LOG")
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
