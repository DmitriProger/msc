mod cli;
mod config;
mod error;
mod server;
mod state;
mod screen;
mod tui;
mod update;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use config::GlobalConfig;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Debug, Parser)]
#[command(
    name = "anvil",
    about = "Anvil - CLI for managing Minecraft servers on Linux using GNU Screen",
    version = config::VERSION,
    disable_help_subcommand = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List all servers and their status
    List,

    /// Show version
    Version,

    /// Update anvil from a GitHub release
    Update(UpdateArgs),

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

fn is_root() -> bool {
    #[cfg(unix)]
    {
        extern "C" {
            fn geteuid() -> u32;
        }
        unsafe { geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Prevent running server management commands as root (UID 0) to avoid screen owner conflicts.
    if is_root() {
        let is_allowed_cmd = match &cli.command {
            Some(Command::Version) | Some(Command::Update(_)) => true,
            _ => false,
        };
        if !is_allowed_cmd {
            let args: Vec<String> = std::env::args().skip(1).collect();
            let subcmd = if args.is_empty() { String::new() } else { format!(" {}", args.join(" ")) };
            eprintln!("Error: Running server management commands as root is not permitted.");
            eprintln!("It creates screen sessions owned by root. Please run commands as the 'minecraft' user instead, for example:");
            eprintln!("  sudo -u minecraft anvil{}", subcmd);
            std::process::exit(1);
        }
    }

    let global_config = GlobalConfig::load()?;

    init_tracing(&global_config);

    match cli.command {
        None => {
            screen::ScreenClient::check_installed().map_err(|e| {
                eprintln!("{}", e);
                e
            })?;
            tui::run_main_tui(global_config)?;
        }
        Some(Command::List) => {
            cli::cmd_list(&global_config)?;
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
        Some(Command::Server(args)) => {
            handle_server_args(args, &global_config)?;
        }
    }

    Ok(())
}

fn handle_server_args(args: Vec<String>, global_config: &GlobalConfig) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: anvil <name> [start|stop|restart|console|status|send <cmd>]");
        std::process::exit(1);
    }

    let name = &args[0];
    let sub = args.get(1).map(|s| s.as_str());

    let servers = server::discover_servers(global_config);

    match sub {
        None => {
            check_server_exists(&servers, name);
            screen::ScreenClient::check_installed().map_err(|e| {
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
        Some(unknown) => {
            eprintln!("Unknown subcommand: {}", unknown);
            eprintln!(
                "Usage: anvil {} [start|stop|restart|console|status|send]",
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

fn init_tracing(config: &GlobalConfig) {
    if std::env::var_os("ANVIL_LOG").is_none() {
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
