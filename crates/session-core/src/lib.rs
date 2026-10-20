//! Platform-independent session state, bounded byte transport and OS resource ownership.
pub mod backend;
pub mod model;
pub mod profile;
pub mod supervisor;
pub mod transport;

#[cfg(windows)]
mod windows_env;
