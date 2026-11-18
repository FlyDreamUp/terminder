use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use terminder_session_core::{
    model::{CoreError, Result},
    profile::{Profile, builtin_profiles},
};

const DEFAULT_SHORTCUTS: [(&str, &str); 20] = [
    ("new_tab", "ctrl-shift-t"),
    ("close_tab", "ctrl-shift-w"),
    ("close_pane", "ctrl-alt-w"),
    ("split_right", "ctrl-alt-r"),
    ("split_down", "ctrl-alt-d"),
    ("focus_left", "alt-left"),
    ("focus_right", "alt-right"),
    ("focus_up", "alt-up"),
    ("focus_down", "alt-down"),
    ("next_tab", "ctrl-tab"),
    ("previous_tab", "ctrl-shift-tab"),
    ("copy", "ctrl-shift-c"),
    ("paste", "ctrl-shift-v"),
    ("find", "ctrl-shift-f"),
    ("settings", "ctrl-,"),
    ("rename_tab", "f2"),
    ("restart", "ctrl-shift-enter"),
    ("zoom_in", "ctrl-="),
    ("zoom_out", "ctrl--"),
    ("zoom_reset", "ctrl-0"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub default_profile: String,
    pub profiles: Vec<Profile>,
    pub terminal: TerminalConfig,
    pub window: WindowConfig,
    pub session: SessionConfig,
    pub clipboard: ClipboardConfig,
    pub shortcuts: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub font_family: Vec<String>,
    pub font_size_px: f32,
    pub theme: String,
    pub scrollback_lines: u32,
}
impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: vec!["Cascadia Mono".into(), "Consolas".into()],
            font_size_px: 14.0,
            theme: "system".into(),
            scrollback_lines: 5000,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    pub restore_geometry: bool,
    pub restore_tabs: bool,
}
impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            restore_geometry: true,
            restore_tabs: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    pub max_sessions: usize,
    pub confirm_close_running: bool,
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: 16,
            confirm_close_running: true,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ClipboardConfig {
    pub confirm_multiline: bool,
    pub max_paste_bytes: usize,
}
impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            confirm_multiline: true,
            max_paste_bytes: 262144,
        }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 2,
            default_profile: "auto".into(),
            profiles: builtin_profiles(),
            terminal: TerminalConfig::default(),
            window: WindowConfig::default(),
            session: SessionConfig::default(),
            clipboard: ClipboardConfig::default(),
            shortcuts: DEFAULT_SHORTCUTS
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 2 {
            return Err(CoreError::new(
                "schema_version",
                "配置版本不受支持；原文件保留且禁止写回",
            ));
        }
        if self.profiles.len() > 64 {
            return Err(CoreError::new("config_limit", "Profile 超过 64 个"));
        }
        let mut ids = HashSet::new();
        for p in &self.profiles {
            p.validate()?;
            if !ids.insert(&p.id) {
                return Err(CoreError::new("duplicate_profile", "Profile 标识重复"));
            }
        }
        if self.default_profile != "auto" && !ids.contains(&self.default_profile) {
            return Err(CoreError::new("default_profile", "默认 Profile 不存在"));
        }
        let t = &self.terminal;
        if !(8.0..=48.0).contains(&t.font_size_px)
            || t.scrollback_lines > 20000
            || t.font_family.is_empty()
            || t.font_family.len() > 8
            || t.font_family
                .iter()
                .any(|f| f.is_empty() || f.len() > 100 || f.contains(['\0', '\n', '\r']))
            || !["system", "dark", "light"].contains(&t.theme.as_str())
        {
            return Err(CoreError::new(
                "terminal_config",
                "字体、字号（8–48）、主题、历史（0–20000）配置无效",
            ));
        }
        if !(1..=16).contains(&self.session.max_sessions)
            || self.session.max_sessions as u32 * t.scrollback_lines > 160000
        {
            return Err(CoreError::new(
                "resource_budget",
                "会话上限须为 1–16，且会话上限 × 历史行数不得超过 160000",
            ));
        }
        if !(1..=262144).contains(&self.clipboard.max_paste_bytes) {
            return Err(CoreError::new("paste_limit", "粘贴限制须为 1–262144 字节"));
        }
        let mut bindings = HashSet::new();
        for (action, key) in &self.shortcuts {
            if !DEFAULT_SHORTCUTS.iter().any(|(name, _)| *name == action)
                || key.is_empty()
                || key.len() > 80
                || !key.is_ascii()
                || key.chars().any(char::is_whitespace)
            {
                return Err(CoreError::new("invalid_shortcut", "快捷键动作或格式无效"));
            }
            #[cfg(windows)]
            let parsed = gpui::Keystroke::parse(key).map_err(|_| {
                CoreError::new(
                    "invalid_shortcut",
                    format!("无效的 GPUI 快捷键：{action} = {key}"),
                )
            })?;
            #[cfg(not(windows))]
            let parsed = key.to_ascii_lowercase();
            if !bindings.insert(parsed) {
                return Err(CoreError::new("invalid_shortcut", "快捷键组合重复"));
            }
        }
        Ok(())
    }
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > 256 * 1024 {
            return Err(CoreError::new("config_size", "配置文件超过 256 KiB"));
        }
        let value: toml::Value = toml::from_str(text)
            .map_err(|e| CoreError::new("config_parse", format!("配置格式错误：{e}")))?;
        if value
            .get("schema_version")
            .is_some_and(|version| version.as_integer() != Some(2))
        {
            return Err(CoreError::new(
                "schema_version",
                "配置版本不受支持；原文件保留且禁止写回",
            ));
        }
        let c: Self = value
            .try_into()
            .map_err(|e| CoreError::new("config_parse", format!("配置格式错误：{e}")))?;
        c.validate()?;
        Ok(c)
    }
}
pub fn default_text() -> String {
    let mut text = String::from(
        "# Terminder configuration\n# Shell 与目录修改只影响新会话；恢复只重建启动环境，不恢复命令。\n\n",
    );
    text.push_str(&toml::to_string_pretty(&Config::default()).expect("default serializable"));
    text
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPane {
    pub id: crate::layout::PaneId,
    pub profile_id: String,
    pub startup_cwd: Option<std::path::PathBuf>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTab {
    pub title: Option<String>,
    pub layout: crate::layout::Layout,
    pub active: crate::layout::PaneId,
    pub panes: Vec<SavedPane>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub schema_version: u32,
    pub geometry: Option<Geometry>,
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            schema_version: 2,
            geometry: None,
            tabs: vec![],
            active_tab: 0,
        }
    }
}
impl Workspace {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 2
            || self.tabs.len() > 16
            || (!self.tabs.is_empty() && self.active_tab >= self.tabs.len())
            || self.geometry.as_ref().is_some_and(|g| {
                g.width < 600 || g.width > 16000 || g.height < 400 || g.height > 16000
            })
        {
            return Err(CoreError::new(
                "workspace_invalid",
                "工作区版本、窗口尺寸或标签数量无效",
            ));
        }
        let mut ids = HashSet::new();
        for tab in &self.tabs {
            let layout_ids = tab.layout.ids();
            if !tab.layout.validate()
                || !layout_ids.contains(&tab.active)
                || tab.panes.len() != layout_ids.len()
                || tab
                    .title
                    .as_ref()
                    .is_some_and(|s| s.chars().count() > 80 || s.chars().any(char::is_control))
            {
                return Err(CoreError::new("workspace_invalid", "分屏布局无效"));
            }
            for pane in &tab.panes {
                if !ids.insert(pane.id)
                    || ids.len() > 16
                    || !layout_ids.contains(&pane.id)
                    || pane.profile_id.is_empty()
                    || pane.profile_id.len() > 64
                    || pane
                        .startup_cwd
                        .as_ref()
                        .is_some_and(|p| !p.is_absolute() || p.as_os_str().len() > 32768)
                {
                    return Err(CoreError::new("workspace_invalid", "窗格启动信息无效"));
                }
            }
        }
        Ok(())
    }
}
