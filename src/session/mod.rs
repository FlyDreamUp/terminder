pub mod command;
pub mod event;
pub mod manager;
pub mod powershell;

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TabId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabState {
    pub id: TabId,
    pub title: String,
    pub cwd: PathBuf,
    pub shell: ShellKind,
    pub status: SessionStatus,
    pub unread_count: u32,
    pub is_renaming: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Pwsh,
    WindowsPowerShell,
    Custom(PathBuf),
}
