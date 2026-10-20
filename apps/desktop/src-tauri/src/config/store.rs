use super::model::*;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use terminder_session_core::model::{CoreError, Result};

#[derive(Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub state: PathBuf,
    pub logs: PathBuf,
}
impl Paths {
    pub fn discover() -> Result<Self> {
        #[cfg(feature = "test-support")]
        if let Some(root) = std::env::var_os("TERMINDER_TEST_DATA_ROOT") {
            let root = PathBuf::from(root);
            if !root.is_absolute() || !root.is_dir() {
                return Err(CoreError::new(
                    "test_path",
                    "Test data root must be an existing absolute directory",
                ));
            }
            return Ok(Self {
                config: root.join("config/config.toml"),
                state: root.join("state/workspace.json"),
                logs: root.join("logs"),
            });
        }
        let base = directories::BaseDirs::new()
            .ok_or_else(|| CoreError::new("app_dirs", "无法定位用户数据目录"))?;
        #[cfg(windows)]
        {
            Ok(Self {
                config: base.config_dir().join("Terminder/config.toml"),
                state: base.data_local_dir().join("Terminder/state/workspace.json"),
                logs: base.data_local_dir().join("Terminder/logs"),
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                config: base.config_dir().join("terminder/config.toml"),
                state: base
                    .state_dir()
                    .unwrap_or(base.data_local_dir())
                    .join("terminder/workspace.json"),
                logs: base
                    .state_dir()
                    .unwrap_or(base.data_local_dir())
                    .join("terminder/logs"),
            })
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDocument {
    pub config: Config,
    pub text: String,
    pub revision: String,
    pub error: Option<CoreError>,
    pub writable: bool,
}
fn revision(bytes: Option<&[u8]>) -> String {
    bytes
        .map(|b| format!("{:x}", Sha256::digest(b)))
        .unwrap_or_else(|| "missing".into())
}
fn read_limited(path: &Path, max: u64) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CoreError::io("read_config", e)),
    };
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| CoreError::io("read_config", e))?;
    if bytes.len() as u64 > max {
        return Err(CoreError::new("file_limit", "配置或快照文件过大"));
    }
    Ok(Some(bytes))
}
pub fn load_config(path: &Path) -> ConfigDocument {
    match read_limited(path, 256 * 1024) {
        Ok(None) => ConfigDocument {
            config: Config::default(),
            text: default_text(),
            revision: "missing".into(),
            error: None,
            writable: true,
        },
        Ok(Some(bytes)) => {
            let rev = revision(Some(&bytes));
            match String::from_utf8(bytes) {
                Ok(text) => match Config::parse(&text) {
                    Ok(config) => ConfigDocument {
                        config,
                        text,
                        revision: rev,
                        error: None,
                        writable: true,
                    },
                    Err(error) => {
                        let writable = error.code != "schema_version";
                        ConfigDocument {
                            config: Config::default(),
                            text,
                            revision: rev,
                            error: Some(error),
                            writable,
                        }
                    }
                },
                Err(_) => ConfigDocument {
                    config: Config::default(),
                    text: String::new(),
                    revision: rev,
                    error: Some(CoreError::new(
                        "config_encoding",
                        "配置不是 UTF-8；请修复原文件",
                    )),
                    writable: false,
                },
            }
        }
        Err(error) => ConfigDocument {
            config: Config::default(),
            text: String::new(),
            revision: "unreadable".into(),
            error: Some(error),
            writable: false,
        },
    }
}

pub fn save_config(path: &Path, text: &str, expected: &str) -> Result<ConfigDocument> {
    Config::parse(text)?;
    let current = load_config(path);
    if !current.writable {
        return Err(CoreError::new(
            "config_readonly",
            "当前配置不可写，请先修复原文件或使用匹配版本",
        ));
    }
    atomic_save(path, text.as_bytes(), expected, |bytes| {
        std::str::from_utf8(bytes).is_ok_and(|text| Config::parse(text).is_ok())
    })?;
    Ok(load_config(path))
}

pub fn update_terminal(
    path: &Path,
    terminal: TerminalConfig,
    expected: &str,
) -> Result<ConfigDocument> {
    let current = load_config(path);
    if current.error.is_some() || !current.writable {
        return Err(CoreError::new(
            "config_invalid",
            "请先修复配置，外观设置未写入",
        ));
    }
    let mut config = current.config.clone();
    config.terminal = terminal.clone();
    config.validate()?;
    let mut doc = current
        .text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| CoreError::new("config_parse", e.to_string()))?;
    fn patch(doc: &mut toml_edit::DocumentMut, key: &str, mut item: toml_edit::Item) {
        if let Some(old) = doc
            .get("terminal")
            .and_then(|t| t.get(key))
            .and_then(toml_edit::Item::as_value)
            && let Some(value) = item.as_value_mut()
        {
            *value.decor_mut() = old.decor().clone();
        }
        doc["terminal"][key] = item;
    }
    let old = &current.config.terminal;
    if terminal.font_family != old.font_family {
        let mut fonts = toml_edit::Array::new();
        for font in terminal.font_family {
            fonts.push(font);
        }
        patch(&mut doc, "font_family", toml_edit::value(fonts));
    }
    if terminal.font_size_px != old.font_size_px {
        patch(
            &mut doc,
            "font_size_px",
            toml_edit::value(f64::from(terminal.font_size_px)),
        );
    }
    if terminal.theme != old.theme {
        patch(&mut doc, "theme", toml_edit::value(terminal.theme));
    }
    if terminal.scrollback_lines != old.scrollback_lines {
        patch(
            &mut doc,
            "scrollback_lines",
            toml_edit::value(i64::from(terminal.scrollback_lines)),
        );
    }
    if terminal.renderer != old.renderer {
        patch(&mut doc, "renderer", toml_edit::value(terminal.renderer));
    }
    save_config(path, &doc.to_string(), expected)
}

pub fn load_workspace(path: &Path) -> Result<Workspace> {
    let Some(bytes) = read_limited(path, 128 * 1024)? else {
        return Ok(Workspace::default());
    };
    let snapshot: Workspace = serde_json::from_slice(&bytes).map_err(|e| {
        CoreError::new(
            "workspace_parse",
            format!("工作区快照损坏，已保留原文件：{e}"),
        )
    })?;
    snapshot.validate()?;
    Ok(snapshot)
}
pub fn file_revision(path: &Path) -> Result<String> {
    Ok(revision(read_limited(path, 256 * 1024)?.as_deref()))
}
pub fn save_workspace(path: &Path, snapshot: &Workspace, expected: &str) -> Result<String> {
    snapshot.validate()?;
    // Unknown/newer or damaged state is retained for diagnosis instead of silently overwriting.
    load_workspace(path)?;
    let bytes = serde_json::to_vec_pretty(snapshot)
        .map_err(|e| CoreError::new("workspace_encode", e.to_string()))?;
    if bytes.len() > 128 * 1024 {
        return Err(CoreError::new(
            "workspace_size",
            "工作区快照超过 128 KiB，原快照已保留",
        ));
    }
    atomic_save(path, &bytes, expected, |bytes| {
        serde_json::from_slice::<Workspace>(bytes).is_ok_and(|snapshot| snapshot.validate().is_ok())
    })?;
    Ok(revision(Some(&bytes)))
}

fn atomic_save(
    path: &Path,
    bytes: &[u8],
    expected: &str,
    valid_previous: fn(&[u8]) -> bool,
) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| CoreError::new("config_path", "配置路径无父目录"))?;
    fs::create_dir_all(dir).map_err(|e| CoreError::io("create_config_dir", e))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))
        .map_err(|e| CoreError::io("write_lock", e))?;
    lock.try_lock_exclusive()
        .map_err(|_| CoreError::new("config_locked", "另一个实例正在保存，请稍后重试"))?;
    let previous = read_limited(path, 256 * 1024)?;
    if expected != revision(previous.as_deref()) {
        return Err(CoreError::new(
            "config_conflict",
            "文件已被其他编辑器或实例修改，请重新加载后合并",
        ));
    }
    let temp = stage_file(dir, bytes)?;
    let result = (|| {
        let recovery = if let Some(previous) = &previous {
            // Repairing a damaged document must not replace the last usable backup with damage.
            // Keep one bounded damaged-source snapshot for manual recovery as well.
            let backup = path.with_extension(if valid_previous(previous) {
                "bak"
            } else {
                "invalid"
            });
            let staged_backup = stage_file(dir, previous)?;
            if let Err(error) = replace(&staged_backup, &backup, backup.exists(), None) {
                let _ = fs::remove_file(&staged_backup);
                return Err(error);
            }
            Some(backup)
        } else {
            None
        };
        replace(&temp, path, previous.is_some(), recovery.as_deref())?;
        #[cfg(unix)]
        File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| CoreError::io("flush_directory", e))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
fn stage_file(dir: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let temp = dir.join(format!(".terminder-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| CoreError::io("create_temporary", e))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| CoreError::io("flush_config", e))
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(temp)
}
fn replace(temp: &Path, target: &Path, exists: bool, recovery: Option<&Path>) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
        };
        let t: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        let s: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let success = unsafe {
            if exists {
                ReplaceFileW(
                    t.as_ptr(),
                    s.as_ptr(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } else {
                MoveFileExW(s.as_ptr(), t.as_ptr(), MOVEFILE_WRITE_THROUGH)
            }
        };
        if success == 0 {
            let error = std::io::Error::last_os_error();
            // Some ReplaceFile failures may move the original. Restore the exact previous source only
            // when the destination vanished; never overwrite a surviving file of uncertain state.
            if !target.exists()
                && let Some(recovery) = recovery
            {
                let _ = fs::copy(recovery, target);
            }
            return Err(CoreError::io("replace_file", error));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (exists, recovery);
        fs::rename(temp, target).map_err(|e| CoreError::io("replace_file", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_preserves_comments_and_detects_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = format!(
            "# user note\n{}",
            default_text().replace("font_size_px = 14.0", "font_size_px = 14.0 # size note")
        );
        let doc = save_config(&path, &text, "missing").unwrap();
        let mut terminal = doc.config.terminal.clone();
        terminal.font_size_px = 18.0;
        let saved = update_terminal(&path, terminal, &doc.revision).unwrap();
        assert!(saved.text.contains("# user note"));
        assert!(saved.text.contains("# size note"));
        assert_eq!(saved.config.terminal.font_size_px, 18.0);
        fs::write(&path, format!("{}\n# external", saved.text)).unwrap();
        assert_eq!(
            save_config(&path, &text, &saved.revision).unwrap_err().code,
            "config_conflict"
        );
        assert!(fs::read_to_string(&path).unwrap().contains("# external"));
    }
    #[test]
    fn newer_schema_and_invalid_config_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "schema_version = 999\n").unwrap();
        let doc = load_config(&path);
        assert!(!doc.writable);
        assert!(save_config(&path, &default_text(), &doc.revision).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "schema_version = 999\n");
        assert!(Config::parse("schema_version=1\nmispelled=true\n").is_err());
    }
    #[test]
    fn write_keeps_previous_valid_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let first = save_config(&path, &default_text(), "missing").unwrap();
        save_config(&path, &format!("{}\n# second", first.text), &first.revision).unwrap();
        assert_eq!(
            fs::read_to_string(path.with_extension("bak")).unwrap(),
            first.text
        );
    }
    #[test]
    fn repairing_damage_keeps_the_valid_backup_and_the_damaged_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let first = save_config(&path, &default_text(), "missing").unwrap();
        save_config(&path, &format!("{}\n# second", first.text), &first.revision).unwrap();
        fs::write(&path, "broken = [").unwrap();
        let damaged = load_config(&path);
        assert!(damaged.error.is_some());
        save_config(&path, &default_text(), &damaged.revision).unwrap();
        assert_eq!(
            fs::read_to_string(path.with_extension("bak")).unwrap(),
            first.text
        );
        assert_eq!(
            fs::read_to_string(path.with_extension("invalid")).unwrap(),
            "broken = ["
        );
        assert!(load_config(&path).error.is_none());
    }

    #[test]
    fn oversized_workspace_is_rejected_before_touching_the_saved_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.json");
        let first = Workspace::default();
        let revision = save_workspace(&path, &first, "missing").unwrap();
        let original = fs::read(&path).unwrap();
        let mut oversized = first;
        oversized.tabs = (0..16)
            .map(|id| SavedTab {
                id: id.to_string(),
                profile_id: "fixture".into(),
                startup_cwd: Some(format!("C:\\{}", "a".repeat(9000))),
                title: None,
            })
            .collect();
        assert_eq!(
            save_workspace(&path, &oversized, &revision)
                .unwrap_err()
                .code,
            "workspace_size"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(load_workspace(&path).is_ok());
    }

    #[test]
    fn budgets_and_shortcuts_are_validated() {
        let mut c = Config::default();
        c.terminal.scrollback_lines = 20000;
        assert!(c.validate().is_err());
        c.session.max_sessions = 8;
        assert!(c.validate().is_ok());
        c.shortcuts.insert("copy".into(), "Shift+Ctrl+T".into());
        assert!(c.validate().is_err());
    }
}
