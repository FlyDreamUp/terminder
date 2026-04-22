use std::path::PathBuf;

use crate::session::TabId;

#[derive(Debug)]
pub enum SessionEvent {
    Started { tab_id: TabId, pid: u32 },
    Output { tab_id: TabId, bytes: Vec<u8> },
    TitleChanged { tab_id: TabId, title: String },
    CwdChanged { tab_id: TabId, cwd: PathBuf },
    Exited { tab_id: TabId, code: Option<i32> },
    Error { tab_id: TabId, message: String },
}
