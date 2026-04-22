use std::path::{Path, PathBuf};

use crate::config::model::ShellConfig;
use crate::error::{AppError, Result};
use crate::session::{ShellKind, powershell::ShellLaunch};

pub fn locate_shell(config: &ShellConfig) -> Result<ShellLaunch> {
    if let Some(path) = &config.explicit_path {
        validate_explicit_path(path)?;
        return Ok(ShellLaunch {
            shell_path: path.clone(),
            shell_kind: ShellKind::Custom(path.clone()),
            startup_args: config.startup_args.clone(),
        });
    }

    if config.prefer_pwsh {
        if let Ok(path) = which::which("pwsh.exe") {
            return Ok(ShellLaunch {
                shell_path: path,
                shell_kind: ShellKind::Pwsh,
                startup_args: config.startup_args.clone(),
            });
        }
    }

    let fallback = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    if fallback.exists() {
        return Ok(ShellLaunch {
            shell_path: fallback,
            shell_kind: ShellKind::WindowsPowerShell,
            startup_args: config.startup_args.clone(),
        });
    }

    if let Ok(path) = which::which("powershell.exe") {
        return Ok(ShellLaunch {
            shell_path: path,
            shell_kind: ShellKind::WindowsPowerShell,
            startup_args: config.startup_args.clone(),
        });
    }

    Err(AppError::ShellLocate(
        "unable to find pwsh.exe or powershell.exe".to_string(),
    ))
}

fn validate_explicit_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(AppError::NonAbsolutePath(path.to_path_buf()));
    }
    if !path.exists() {
        return Err(AppError::ShellLocate(format!(
            "explicit shell path does not exist: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_explicit_path;

    #[test]
    fn reject_relative_path() {
        let err = validate_explicit_path(std::path::Path::new("pwsh.exe")).unwrap_err();
        assert!(err.to_string().contains("not absolute"));
    }
}
