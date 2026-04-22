use std::collections::HashSet;

use crate::config::model::ShortcutConfig;

pub fn detect_conflicts(shortcuts: &ShortcutConfig) -> Vec<String> {
    let keys = [
        &shortcuts.new_tab,
        &shortcuts.close_tab,
        &shortcuts.next_tab,
        &shortcuts.previous_tab,
        &shortcuts.copy,
        &shortcuts.paste,
        &shortcuts.settings,
    ];
    let mut seen = HashSet::new();
    let mut conflicts = Vec::new();
    for key in keys {
        if !seen.insert(key.to_ascii_lowercase()) {
            conflicts.push(key.clone());
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use crate::config::model::ShortcutConfig;

    use super::detect_conflicts;

    #[test]
    fn detect_duplicate_shortcuts() {
        let config = ShortcutConfig {
            new_tab: "Ctrl+A".to_string(),
            close_tab: "Ctrl+A".to_string(),
            next_tab: "Ctrl+Tab".to_string(),
            previous_tab: "Ctrl+Shift+Tab".to_string(),
            copy: "Ctrl+Shift+C".to_string(),
            paste: "Ctrl+Shift+V".to_string(),
            settings: "Ctrl+,".to_string(),
        };
        let duplicates = detect_conflicts(&config);
        assert_eq!(duplicates.len(), 1);
    }
}
