use crate::app::AppController;
use crate::error::{AppError, Result};
use crate::shortcuts;

pub fn parse_font_size(value: &str) -> Result<f32> {
    let parsed = value
        .trim()
        .parse::<f32>()
        .map_err(|_| AppError::Config("font size must be a number".to_string()))?;
    if parsed < 8.0 || parsed > 48.0 {
        return Err(AppError::Config(
            "font size should be between 8 and 48".to_string(),
        ));
    }
    Ok(parsed)
}

pub fn parse_scrollback(value: &str) -> Result<usize> {
    let parsed = value
        .trim()
        .parse::<usize>()
        .map_err(|_| AppError::Config("scrollback must be an integer".to_string()))?;
    if !(100..=200_000).contains(&parsed) {
        return Err(AppError::Config(
            "scrollback must be between 100 and 200000".to_string(),
        ));
    }
    Ok(parsed)
}

pub fn apply_terminal_settings(
    app: &mut AppController,
    font_size: f32,
    scrollback_lines: usize,
) -> Result<()> {
    app.config.terminal.font_size = font_size;
    app.config.terminal.scrollback_lines = scrollback_lines;

    let conflicts = shortcuts::detect_conflicts(&app.config.shortcuts);
    if !conflicts.is_empty() {
        return Err(AppError::Config(format!(
            "shortcut conflict detected: {}",
            conflicts.join(", ")
        )));
    }

    app.save_config()
}
