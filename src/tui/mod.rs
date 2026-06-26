pub mod main_screen;
pub mod server_screen;
pub mod widgets;

use crate::config::GlobalConfig;
use anyhow::Result;

pub fn run_main_tui(config: GlobalConfig) -> Result<()> {
    main_screen::run(config)
}

pub fn run_server_tui(server_name: String, config: GlobalConfig) -> Result<()> {
    server_screen::run(server_name, config)
}
