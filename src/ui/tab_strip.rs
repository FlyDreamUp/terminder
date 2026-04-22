use crate::session::{SessionStatus, TabState};

pub fn tab_label(tab: &TabState, is_active: bool) -> String {
    let mut name = tab.title.clone();
    if !is_active && tab.unread_count > 0 {
        name.push_str(&format!(" *{}", tab.unread_count));
    }
    if matches!(tab.status, SessionStatus::Failed { .. }) {
        name.push_str(" !");
    }
    name
}
