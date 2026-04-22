pub mod app_shell_native;
pub mod settings;
pub mod status_bar;
pub mod tab_strip;
pub mod terminal_pane;

use std::sync::OnceLock;

use crate::config::model::AppConfig;
use crate::config::store::ConfigStore;
use crate::error::Result;
use crate::session::powershell::ShellLaunch;

#[derive(Clone)]
pub struct UiBootstrap {
    pub config_store: ConfigStore,
    pub config: AppConfig,
    pub shell: ShellLaunch,
}

static BOOTSTRAP: OnceLock<UiBootstrap> = OnceLock::new();

pub fn bootstrap() -> &'static UiBootstrap {
    BOOTSTRAP.get().expect("ui bootstrap is not initialized")
}

pub fn run(config_store: ConfigStore, config: AppConfig, shell: ShellLaunch) -> Result<()> {
    let _ = BOOTSTRAP.set(UiBootstrap {
        config_store,
        config,
        shell,
    });

    #[allow(unreachable_code)]
    {
        dioxus::launch(app_shell_native::app_shell);
        Ok(())
    }
}
