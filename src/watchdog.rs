use crate::config::GlobalConfig;
use crate::server::{control::ServerController, discover_servers};
use crate::state::{AppState, DesiredState};
use crate::tmux::TmuxClient;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::time::{Duration, Instant};

const MONITOR_INTERVAL: Duration = Duration::from_secs(10);
const UPTIME_RESET_THRESHOLD: Duration = Duration::from_secs(60);

struct ServerWatchState {
    restart_attempts: u32,
    gave_up: bool,
    start_time: Option<Instant>,
}

impl ServerWatchState {
    fn new() -> Self {
        Self {
            restart_attempts: 0,
            gave_up: false,
            start_time: None,
        }
    }
}

pub fn run_watchdog(config: GlobalConfig) -> anyhow::Result<()> {
    let _lock = acquire_lock(&config)?;
    tracing::info!("Watchdog started");

    let tmux = TmuxClient::new(&config.tmux_socket);
    let servers = discover_servers(&config);
    tracing::info!("found {} servers", servers.len());

    let state_path = config.state_path();
    let mut state = AppState::load(&state_path).unwrap_or_default();

    // On startup: bring up servers with desired=running
    {
        let controller = ServerController::new(&tmux, &config);
        for server in &servers {
            if state.get_desired(&server.name) == Some(&DesiredState::Running)
                && !controller.is_online(server)
            {
                tracing::info!(server = %server.name, reason = "state_restore", "Starting server");
                if let Err(e) = controller.start(server, &mut state) {
                    tracing::error!(server = %server.name, error = %e, "Failed to start server on boot");
                }
            }
        }
    }

    let mut watch_states: HashMap<String, ServerWatchState> = HashMap::new();
    for server in &servers {
        watch_states.insert(server.name.clone(), ServerWatchState::new());
    }

    loop {
        std::thread::sleep(MONITOR_INTERVAL);

        let servers = discover_servers(&config);
        state = AppState::load(&state_path).unwrap_or_default();
        let controller = ServerController::new(&tmux, &config);

        for server in &servers {
            let desired = state.get_desired(&server.name);
            if desired != Some(&DesiredState::Running) {
                continue;
            }

            let watch = watch_states
                .entry(server.name.clone())
                .or_insert_with(ServerWatchState::new);

            if controller.is_online(server) {
                // Server is running; update uptime tracker and maybe reset counter.
                if watch.start_time.is_none() {
                    watch.start_time = Some(Instant::now());
                }
                if let Some(st) = watch.start_time {
                    if st.elapsed() >= UPTIME_RESET_THRESHOLD && watch.restart_attempts > 0 {
                        tracing::info!(server = %server.name, "Server stable, resetting restart counter");
                        watch.restart_attempts = 0;
                        watch.gave_up = false;
                    }
                }
                continue;
            }

            // Server is down
            watch.start_time = None;

            if !server.config.server.auto_restart {
                tracing::debug!(server = %server.name, "auto_restart is disabled, skipping restart");
                continue;
            }

            if watch.gave_up {
                tracing::debug!(server = %server.name, "Watchdog gave up on this server, skipping");
                continue;
            }

            let max_attempts = server.config.server.max_restart_attempts;
            if watch.restart_attempts >= max_attempts {
                tracing::error!(
                    server = %server.name,
                    attempts = watch.restart_attempts,
                    "Max restart attempts exceeded, giving up"
                );
                watch.gave_up = true;
                continue;
            }

            watch.restart_attempts += 1;
            tracing::warn!(
                server = %server.name,
                attempt = watch.restart_attempts,
                max = max_attempts,
                "Server crashed, scheduling restart"
            );

            let delay = server.config.server.restart_delay_secs;
            std::thread::sleep(Duration::from_secs(delay));

            let mut state_copy = state.clone();
            if let Err(e) = controller.start(server, &mut state_copy) {
                tracing::error!(server = %server.name, error = %e, "Watchdog failed to restart server");
            } else {
                tracing::info!(server = %server.name, reason = "watchdog", "Server restarted");
                watch.start_time = Some(Instant::now());
                state = state_copy;
            }
        }
    }
}

fn acquire_lock(config: &GlobalConfig) -> anyhow::Result<File> {
    let lock_path = config.watchdog_lock_path();
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let ret = unsafe { libc_flock(fd, 6) }; // LOCK_EX | LOCK_NB = 6
    if ret != 0 {
        anyhow::bail!(
            "Another watchdog instance is already running. Lock: {}",
            lock_path.display()
        );
    }
    Ok(file)
}

unsafe fn libc_flock(fd: i32, operation: i32) -> i32 {
    #[link(name = "c")]
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    flock(fd, operation)
}
