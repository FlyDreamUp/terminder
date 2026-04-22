pub fn format_output(raw: &str) -> String {
    if raw.is_empty() {
        return "Terminal is ready. Type directly in the pane.".to_string();
    }

    const MAX_CHARS: usize = 120_000;
    if raw.chars().count() <= MAX_CHARS {
        return raw.to_string();
    }

    let skip = raw.chars().count().saturating_sub(MAX_CHARS);
    let trimmed: String = raw.chars().skip(skip).collect();
    format!("...\n{trimmed}")
}
