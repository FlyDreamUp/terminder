mod app;
mod config;
mod error;
mod logging;
mod platform;
mod session;
mod shortcuts;
mod terminal;
mod ui;

use config::store::ConfigStore;
use error::Result;
use platform::windows::locate_shell;

fn main() {
    if let Err(err) = run() {
        eprintln!("Terminder failed: {err}");
    }
}

fn run() -> Result<()> {
    logging::init_logging();

    let config_store = ConfigStore::default_location()?;
    let config = config_store.load_or_create()?;
    let shell_launch = locate_shell(&config.shell)?;

    ui::run(config_store, config, shell_launch)
}
