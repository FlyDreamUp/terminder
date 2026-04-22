use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub shell: ShellConfig,
    pub terminal: TerminalConfig,
    pub window: WindowConfig,
    pub shortcuts: ShortcutConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShellConfig {
    pub explicit_path: Option<PathBuf>,
    pub prefer_pwsh: bool,
    pub startup_args: Vec<String>,
    pub startup_dir: StartupDir,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum StartupDir {
    CurrentTab,
    Home,
    Custom(PathBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerminalConfig {
    pub font_family: String,
    pub font_size: f32,
    pub scrollback_lines: usize,
    pub theme: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowConfig {
    pub restore_last_size: bool,
    pub restore_last_tabs: bool,
    pub confirm_close_running_tab: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortcutConfig {
    pub new_tab: String,
    pub close_tab: String,
    pub next_tab: String,
    pub previous_tab: String,
    pub copy: String,
    pub paste: String,
    pub settings: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WindowState {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub last_cwd: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shell: ShellConfig {
                explicit_path: None,
                prefer_pwsh: true,
                startup_args: vec!["-NoLogo".to_string()],
                startup_dir: StartupDir::CurrentTab,
            },
            terminal: TerminalConfig {
                font_family: "Cascadia Mono".to_string(),
                font_size: 13.0,
                scrollback_lines: 10_000,
                theme: "system".to_string(),
            },
            window: WindowConfig {
                restore_last_size: true,
                restore_last_tabs: false,
                confirm_close_running_tab: true,
            },
            shortcuts: ShortcutConfig::default(),
        }
    }
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            new_tab: "Ctrl+Shift+T".to_string(),
            close_tab: "Ctrl+Shift+W".to_string(),
            next_tab: "Ctrl+Tab".to_string(),
            previous_tab: "Ctrl+Shift+Tab".to_string(),
            copy: "Ctrl+Shift+C".to_string(),
            paste: "Ctrl+Shift+V".to_string(),
            settings: "Ctrl+,".to_string(),
        }
    }
}
