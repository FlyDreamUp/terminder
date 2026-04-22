use crate::session::{SessionStatus, ShellKind, TabState};

pub fn build_status(tab: &TabState) -> String {
    let shell = match &tab.shell {
        ShellKind::Pwsh => "pwsh".to_string(),
        ShellKind::WindowsPowerShell => "powershell".to_string(),
        ShellKind::Custom(path) => path.display().to_string(),
    };
    let status = match &tab.status {
        SessionStatus::Starting => "Starting".to_string(),
        SessionStatus::Running => "Running".to_string(),
        SessionStatus::Exited { code } => format!("Exited({code:?})"),
        SessionStatus::Failed { message } => format!("Failed({message})"),
    };

    format!("{shell} | {} | {}", tab.cwd.display(), status)
}
