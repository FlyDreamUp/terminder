use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

pub const MAX_SESSIONS: usize = 16;
pub const OUTPUT_CHUNK: usize = 32 * 1024;
pub const INPUT_CHUNK: usize = 16 * 1024;
pub const INPUT_BUDGET: usize = 512 * 1024;
pub const OUTPUT_QUEUE_BYTES: usize = 1024 * 1024;
pub const OUTPUT_QUEUE_BLOCKS: usize = 128;
pub const OUTPUT_WINDOW_BYTES: usize = 256 * 1024;
pub const OUTPUT_WINDOW_BLOCKS: usize = 64;
pub const GLOBAL_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{message}")]
pub struct CoreError {
    pub code: String,
    pub message: String,
    pub stage: String,
    pub os_code: Option<i32>,
}
impl CoreError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            stage: code.into(),
            os_code: None,
        }
    }
    pub fn io(stage: &str, error: std::io::Error) -> Self {
        // Do not embed command lines, environment values or terminal bytes in diagnostics.
        Self {
            code: "os_error".into(),
            message: format!("系统操作失败（{stage}）：{error}"),
            stage: stage.into(),
            os_code: error.raw_os_error(),
        }
    }
}
pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}
impl TerminalSize {
    pub fn validate(self) -> Result<Self> {
        if self.cols == 0
            || self.rows == 0
            || self.cols > 1000
            || self.rows > 1000
            || u32::from(self.cols) * u32::from(self.rows) > 250_000
        {
            return Err(CoreError::new(
                "invalid_size",
                "终端尺寸必须为 1–1000，且总单元格不超过 250000",
            ));
        }
        Ok(self)
    }
}
impl Default for TerminalSize {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub session_id: String,
    pub view_epoch: String,
}
impl Binding {
    pub fn fresh() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            view_epoch: uuid::Uuid::new_v4().to_string(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if uuid::Uuid::parse_str(&self.session_id).is_err()
            || uuid::Uuid::parse_str(&self.view_epoch).is_err()
        {
            return Err(CoreError::new("invalid_binding", "会话标识或视图标识无效"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Starting,
    Running,
    Draining,
    Closing,
    Finalizing,
    Finished,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Exited { code: u32 },
    ClosedByUser,
    SpawnFailed,
    IoFailed,
    ViewLost,
    TimedOut,
}

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub binding: Binding,
    pub phase: Phase,
    pub reason: Option<FinishReason>,
    pub exit_code: Option<u32>,
    pub tail_complete: bool,
    pub cleanup_error: Option<CoreError>,
    pub size: TerminalSize,
}
#[derive(Debug, Clone)]
pub enum SessionEvent {
    State {
        snapshot: Box<SessionSnapshot>,
    },
    OutputEnd {
        sequence: u64,
        bytes: u64,
        truncated: bool,
    },
    Error {
        error: CoreError,
    },
}

#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub profile_id: String,
    pub profile_name: String,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: BTreeMap<OsString, Option<OsString>>,
    pub cwd: PathBuf,
    pub size: TerminalSize,
}
impl SpawnSpec {
    pub fn validate(&self) -> Result<()> {
        self.size.validate()?;
        if !self.program.is_absolute() || !self.program.is_file() {
            return Err(CoreError::new(
                "program_missing",
                "程序必须是存在的绝对文件路径",
            ));
        }
        if !self.cwd.is_absolute() || !self.cwd.is_dir() {
            return Err(CoreError::new(
                "invalid_cwd",
                "启动目录不存在或不是绝对目录",
            ));
        }
        if self.args.len() > 128 {
            return Err(CoreError::new("invalid_args", "参数数量超过 128"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_dimensions() {
        for (cols, rows) in [(0, 24), (80, 0), (1001, 24), (1000, 1000)] {
            assert!(TerminalSize { cols, rows }.validate().is_err());
        }
        assert!(
            TerminalSize {
                cols: 240,
                rows: 80
            }
            .validate()
            .is_ok()
        );
    }
}
