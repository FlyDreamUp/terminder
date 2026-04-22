use std::fs;
use std::path::PathBuf;

use tracing::{info, warn};

use crate::config::model::{AppConfig, WindowState};
use crate::error::{AppError, Result};

#[derive(Clone, Debug)]
pub struct ConfigStore {
    config_path: PathBuf,
    window_state_path: PathBuf,
}

impl ConfigStore {
    pub fn default_location() -> Result<Self> {
        let base_dir = dirs::config_dir()
            .ok_or_else(|| AppError::Config("cannot resolve config dir".into()))?;
        let app_dir = base_dir.join("Terminder");
        let config_path = app_dir.join("config.toml");
        let window_state_path = app_dir.join("window_state.toml");
        Ok(Self {
            config_path,
            window_state_path,
        })
    }

    pub fn load_or_create(&self) -> Result<AppConfig> {
        if !self.config_path.exists() {
            let config = AppConfig::default();
            self.save(&config)?;
            return Ok(config);
        }
        self.load()
    }

    pub fn load(&self) -> Result<AppConfig> {
        let raw = fs::read_to_string(&self.config_path)?;
        match toml::from_str::<AppConfig>(&raw) {
            Ok(config) => Ok(config),
            Err(err) => {
                warn!("invalid config, falling back to default: {err}");
                let config = AppConfig::default();
                self.save(&config)?;
                Ok(config)
            }
        }
    }

    pub fn save(&self, config: &AppConfig) -> Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = toml::to_string_pretty(config)?;
        fs::write(&self.config_path, encoded)?;
        info!("saved config to {}", self.config_path.display());
        Ok(())
    }

    pub fn load_window_state(&self) -> Result<WindowState> {
        if !self.window_state_path.exists() {
            return Ok(WindowState::default());
        }
        let raw = fs::read_to_string(&self.window_state_path)?;
        match toml::from_str::<WindowState>(&raw) {
            Ok(state) => Ok(state),
            Err(err) => {
                warn!("invalid window state, fallback default: {err}");
                Ok(WindowState::default())
            }
        }
    }

    pub fn save_window_state(&self, state: &WindowState) -> Result<()> {
        if let Some(parent) = self.window_state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = toml::to_string_pretty(state)?;
        fs::write(&self.window_state_path, encoded)?;
        Ok(())
    }
}
