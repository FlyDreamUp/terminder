#[cfg(windows)]
mod app;
#[cfg(windows)]
mod commands;
pub mod config;
pub mod platform;
#[cfg(windows)]
pub use app::run;
#[cfg(not(windows))]
pub fn run() {
    eprintln!(
        "Terminder: this release implements Windows ConPTY only. macOS/Linux are not yet supported."
    );
}
