use std::collections::HashMap;
use std::path::PathBuf;

use tracing::warn;

use crate::config::model::{AppConfig, StartupDir, WindowState};
use crate::config::store::ConfigStore;
use crate::error::{AppError, Result};
use crate::session::command::SessionCommand;
use crate::session::event::SessionEvent;
use crate::session::manager::SessionManager;
use crate::session::powershell::ShellLaunch;
use crate::session::{SessionStatus, TabId, TabState};
use crate::terminal::screen::TerminalScreen;

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 34;

pub struct AppState {
    pub tabs: Vec<TabState>,
    pub active_tab: Option<TabId>,
    pub show_settings: bool,
    pub last_error: Option<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            show_settings: false,
            last_error: None,
        }
    }

    fn tab_mut(&mut self, tab_id: TabId) -> Option<&mut TabState> {
        self.tabs.iter_mut().find(|tab| tab.id == tab_id)
    }
}

pub struct AppController {
    config_store: ConfigStore,
    pub config: AppConfig,
    pub window_state: WindowState,
    pub state: AppState,
    session_manager: SessionManager,
    terminal_renderers: HashMap<TabId, TerminalScreen>,
}

impl AppController {
    pub fn bootstrap(
        config_store: ConfigStore,
        config: AppConfig,
        shell: ShellLaunch,
    ) -> Result<Self> {
        let window_state = config_store.load_window_state()?;
        let mut this = Self {
            config_store,
            config,
            window_state,
            state: AppState::new(),
            session_manager: SessionManager::new(shell),
            terminal_renderers: HashMap::new(),
        };
        this.new_tab(None)?;
        Ok(this)
    }

    pub fn new_tab(&mut self, requested_cwd: Option<PathBuf>) -> Result<TabId> {
        let cwd = self.resolve_startup_dir(requested_cwd);
        let tab = self.session_manager.create_tab(cwd)?;
        let tab_id = tab.id;
        self.terminal_renderers.insert(
            tab_id,
            TerminalScreen::new(
                DEFAULT_ROWS,
                DEFAULT_COLS,
                self.config.terminal.scrollback_lines,
            ),
        );
        self.state.tabs.push(tab);
        self.state.active_tab = Some(tab_id);
        self.resize_tab(tab_id, DEFAULT_COLS, DEFAULT_ROWS)?;
        Ok(tab_id)
    }

    pub fn close_tab(&mut self, tab_id: TabId, force: bool) -> Result<()> {
        self.session_manager.close(tab_id, force)?;
        self.state.tabs.retain(|tab| tab.id != tab_id);
        self.terminal_renderers.remove(&tab_id);
        if self.state.tabs.is_empty() {
            let _ = self.new_tab(None)?;
        }
        if self.state.active_tab == Some(tab_id) {
            self.state.active_tab = self.state.tabs.first().map(|tab| tab.id);
        }
        Ok(())
    }

    pub fn switch_tab(&mut self, tab_id: TabId) {
        if self.state.tabs.iter().any(|tab| tab.id == tab_id) {
            self.state.active_tab = Some(tab_id);
            if let Some(tab) = self.state.tab_mut(tab_id) {
                tab.unread_count = 0;
            }
        }
    }

    pub fn rename_tab(&mut self, tab_id: TabId, title: String) {
        if let Some(tab) = self.state.tab_mut(tab_id) {
            tab.title = title.clone();
            let _ = self
                .session_manager
                .send(SessionCommand::SetTitle { tab_id, title });
        }
    }

    pub fn send_bytes_to_active(&mut self, bytes: Vec<u8>) -> Result<()> {
        let tab_id = self
            .state
            .active_tab
            .ok_or_else(|| AppError::Session("no active tab".to_string()))?;
        self.session_manager
            .send(SessionCommand::Write { tab_id, bytes })?;
        Ok(())
    }

    pub fn resize_active(&mut self, cols: u16, rows: u16) -> Result<()> {
        let tab_id = self
            .state
            .active_tab
            .ok_or_else(|| AppError::Session("no active tab".to_string()))?;
        self.resize_tab(tab_id, cols, rows)
    }

    pub fn poll_events(&mut self) -> bool {
        let mut changed = false;
        let events = self.session_manager.drain_events();
        for event in events {
            changed = true;
            self.handle_event(event);
        }
        changed
    }

    pub fn terminal_output(&self, tab_id: TabId) -> String {
        self.terminal_renderers
            .get(&tab_id)
            .map(TerminalScreen::contents)
            .unwrap_or_default()
    }

    pub fn active_tab(&self) -> Option<TabState> {
        let active = self.state.active_tab?;
        self.state.tabs.iter().find(|tab| tab.id == active).cloned()
    }

    pub fn save_config(&self) -> Result<()> {
        self.config_store.save(&self.config)
    }

    pub fn shutdown(&mut self) {
        if let Some(active) = self.active_tab() {
            self.window_state.last_cwd = Some(active.cwd);
        }
        if let Err(err) = self.config_store.save_window_state(&self.window_state) {
            warn!("failed to save window state: {err}");
        }
        self.session_manager.close_all();
    }

    fn resize_tab(&mut self, tab_id: TabId, cols: u16, rows: u16) -> Result<()> {
        if let Some(renderer) = self.terminal_renderers.get_mut(&tab_id) {
            renderer.resize(cols, rows);
        }
        self.session_manager
            .send(SessionCommand::Resize { tab_id, cols, rows })
    }

    fn handle_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Started { tab_id, pid } => {
                if let Some(tab) = self.state.tab_mut(tab_id) {
                    tab.status = SessionStatus::Running;
                    tab.title = format!("{} ({pid})", tab.title);
                }
            }
            SessionEvent::Output { tab_id, bytes } => {
                let renderer = self.terminal_renderers.entry(tab_id).or_insert_with(|| {
                    TerminalScreen::new(
                        DEFAULT_ROWS,
                        DEFAULT_COLS,
                        self.config.terminal.scrollback_lines,
                    )
                });
                renderer.process(&bytes);
                if self.state.active_tab != Some(tab_id) {
                    if let Some(tab) = self.state.tab_mut(tab_id) {
                        tab.unread_count = tab.unread_count.saturating_add(1);
                    }
                }
            }
            SessionEvent::TitleChanged { tab_id, title } => {
                if let Some(tab) = self.state.tab_mut(tab_id) {
                    tab.title = title;
                }
            }
            SessionEvent::CwdChanged { tab_id, cwd } => {
                if let Some(tab) = self.state.tab_mut(tab_id) {
                    tab.cwd = cwd;
                }
            }
            SessionEvent::Exited { tab_id, code } => {
                if let Some(tab) = self.state.tab_mut(tab_id) {
                    tab.status = SessionStatus::Exited { code };
                }
            }
            SessionEvent::Error { tab_id, message } => {
                self.state.last_error = Some(message.clone());
                if let Some(tab) = self.state.tab_mut(tab_id) {
                    tab.status = SessionStatus::Failed { message };
                }
            }
        }
    }

    fn resolve_startup_dir(&self, requested: Option<PathBuf>) -> PathBuf {
        if let Some(path) = requested {
            return path;
        }

        if let Some(active) = self.active_tab() {
            return active.cwd;
        }

        match &self.config.shell.startup_dir {
            StartupDir::CurrentTab => default_home_dir(),
            StartupDir::Home => default_home_dir(),
            StartupDir::Custom(path) => path.clone(),
        }
    }
}

fn default_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\"))
}
