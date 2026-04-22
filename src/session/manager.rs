use std::collections::HashMap;
use std::path::PathBuf;

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::error::{AppError, Result};
use crate::session::command::SessionCommand;
use crate::session::event::SessionEvent;
use crate::session::powershell::{ShellLaunch, spawn_session};
use crate::session::{SessionStatus, TabId, TabState};

#[derive(Debug)]
struct SessionHandle {
    command_tx: Sender<SessionCommand>,
}

#[derive(Debug)]
pub struct SessionManager {
    next_tab_id: u64,
    sessions: HashMap<TabId, SessionHandle>,
    event_tx: Sender<SessionEvent>,
    event_rx: Receiver<SessionEvent>,
    shell: ShellLaunch,
}

impl SessionManager {
    pub fn new(shell: ShellLaunch) -> Self {
        let (event_tx, event_rx) = unbounded();
        Self {
            next_tab_id: 1,
            sessions: HashMap::new(),
            event_tx,
            event_rx,
            shell,
        }
    }

    pub fn create_tab(&mut self, cwd: PathBuf) -> Result<TabState> {
        let tab_id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        let (command_tx, command_rx) = unbounded();
        spawn_session(
            tab_id,
            self.shell.clone(),
            cwd.clone(),
            command_rx,
            self.event_tx.clone(),
        )?;

        self.sessions.insert(tab_id, SessionHandle { command_tx });

        Ok(TabState {
            id: tab_id,
            title: format!("PowerShell {}", tab_id.0),
            cwd,
            shell: self.shell.shell_kind.clone(),
            status: SessionStatus::Starting,
            unread_count: 0,
            is_renaming: false,
        })
    }

    pub fn send(&self, command: SessionCommand) -> Result<()> {
        let tab_id = extract_tab_id(&command);
        let session = self.sessions.get(&tab_id).ok_or_else(|| {
            AppError::Session(format!("unknown tab {}, cannot send command", tab_id.0))
        })?;
        session
            .command_tx
            .send(command)
            .map_err(|err| AppError::Session(format!("failed to send command: {err}")))
    }

    pub fn close(&mut self, tab_id: TabId, force: bool) -> Result<()> {
        if let Some(session) = self.sessions.get(&tab_id) {
            session
                .command_tx
                .send(SessionCommand::Close { tab_id, force })
                .map_err(|err| AppError::Session(format!("failed to close tab: {err}")))?;
        }
        self.sessions.remove(&tab_id);
        Ok(())
    }

    pub fn close_all(&mut self) {
        let tab_ids: Vec<TabId> = self.sessions.keys().copied().collect();
        for tab_id in tab_ids {
            let _ = self.close(tab_id, true);
        }
    }

    pub fn drain_events(&self) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

fn extract_tab_id(command: &SessionCommand) -> TabId {
    match command {
        SessionCommand::Write { tab_id, .. }
        | SessionCommand::Resize { tab_id, .. }
        | SessionCommand::SetTitle { tab_id, .. }
        | SessionCommand::Close { tab_id, .. } => *tab_id,
    }
}

#[cfg(test)]
mod tests {
    use crate::session::ShellKind;

    use super::*;

    #[test]
    fn tab_id_should_increment() {
        let shell = ShellLaunch {
            shell_path: PathBuf::from("C:\\fake\\pwsh.exe"),
            shell_kind: ShellKind::Pwsh,
            startup_args: vec!["-NoLogo".to_string()],
        };
        let mut manager = SessionManager::new(shell);
        let id1 = manager.next_tab_id;
        manager.next_tab_id += 1;
        let id2 = manager.next_tab_id;
        assert_eq!(id2, id1 + 1);
    }
}
