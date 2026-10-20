use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_SESSIONS: usize = 16;
pub const OUTPUT_CHUNK: usize = 32 * 1024;
pub const INPUT_CHUNK: usize = 16 * 1024;
pub const INPUT_BUDGET: usize = 512 * 1024;
pub const OUTPUT_QUEUE_BYTES: usize = 1024 * 1024;
pub const OUTPUT_QUEUE_BLOCKS: usize = 128;
pub const OUTPUT_WINDOW_BYTES: usize = 256 * 1024;
pub const OUTPUT_WINDOW_BLOCKS: usize = 64;
pub const GLOBAL_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub session_id: String,
    pub view_epoch: String,
    pub protocol_version: u32,
}
impl Binding {
    pub fn fresh() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            view_epoch: uuid::Uuid::new_v4().to_string(),
            protocol_version: PROTOCOL_VERSION,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.protocol_version != PROTOCOL_VERSION
            || uuid::Uuid::parse_str(&self.session_id).is_err()
            || uuid::Uuid::parse_str(&self.view_epoch).is_err()
        {
            return Err(CoreError::new("invalid_binding", "会话标识或协议版本无效"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Starting,
    Running,
    Draining,
    Closing,
    Finalizing,
    Finished,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinishReason {
    Exited { code: u32 },
    ClosedByUser,
    SpawnFailed,
    IoFailed,
    ViewLost,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub binding: Binding,
    pub profile_id: String,
    pub profile_name: String,
    pub startup_cwd: Option<String>,
    pub cwd_source: String,
    pub phase: Phase,
    pub reason: Option<FinishReason>,
    pub exit_code: Option<u32>,
    pub tail_complete: bool,
    pub congested: bool,
    pub cleanup_error: Option<CoreError>,
    pub size: TerminalSize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    State {
        snapshot: Box<SessionSnapshot>,
    },
    OutputEnd {
        sequence: String,
        bytes: String,
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
    pub cwd_source: String,
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

pub fn parse_counter(s: &str) -> Result<u64> {
    if s.is_empty()
        || s.len() > 20
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(CoreError::new(
            "invalid_counter",
            "计数必须是规范的无符号十进制字符串",
        ));
    }
    s.parse()
        .map_err(|_| CoreError::new("invalid_counter", "计数超出 64 位范围"))
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
    #[test]
    fn counters_are_lossless_and_canonical() {
        assert_eq!(parse_counter("18446744073709551615").unwrap(), u64::MAX);
        for bad in ["", "01", "+1", "1.0", "18446744073709551616", "-1"] {
            assert!(parse_counter(bad).is_err());
        }
    }
}
