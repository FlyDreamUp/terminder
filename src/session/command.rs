use crate::session::TabId;

#[derive(Debug)]
pub enum SessionCommand {
    Write { tab_id: TabId, bytes: Vec<u8> },
    Resize { tab_id: TabId, cols: u16, rows: u16 },
    SetTitle { tab_id: TabId, title: String },
    Close { tab_id: TabId, force: bool },
}
