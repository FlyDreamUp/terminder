use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use terminder_session_core::{
    model::{CoreError, Result},
    profile::{Profile, builtin_profiles},
};

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
    pub renderer: String,
}
impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: vec![
                "Cascadia Mono".into(),
                "Consolas".into(),
                "monospace".into(),
            ],
            font_size_px: 14.0,
            theme: "system".into(),
            scrollback_lines: 5000,
            renderer: "auto".into(),
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
            schema_version: 1,
            default_profile: "auto".into(),
            profiles: builtin_profiles(),
            terminal: TerminalConfig::default(),
            window: WindowConfig::default(),
            session: SessionConfig::default(),
            clipboard: ClipboardConfig::default(),
            shortcuts: [
                ("new_tab", "Ctrl+Shift+T"),
                ("close_tab", "Ctrl+Shift+W"),
                ("close_pane", "Ctrl+Alt+W"),
                ("split_right", "Ctrl+Alt+R"),
                ("split_down", "Ctrl+Alt+D"),
                ("focus_left", "Alt+ArrowLeft"),
                ("focus_right", "Alt+ArrowRight"),
                ("focus_up", "Alt+ArrowUp"),
                ("focus_down", "Alt+ArrowDown"),
                ("toggle_pane_zoom", "Ctrl+Alt+Z"),
                ("next_tab", "Ctrl+Tab"),
                ("previous_tab", "Ctrl+Shift+Tab"),
                ("copy", "Ctrl+Shift+C"),
                ("paste", "Ctrl+Shift+V"),
                ("find", "Ctrl+Shift+F"),
                ("settings", "Ctrl+,"),
                ("zoom_in", "Ctrl+="),
                ("zoom_out", "Ctrl+-"),
                ("zoom_reset", "Ctrl+0"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
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
            || !["auto", "dom"].contains(&t.renderer.as_str())
        {
            return Err(CoreError::new(
                "terminal_config",
                "字体、字号（8–48）、主题、历史（0–20000）或渲染器配置无效",
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
        let actions = Config::default().shortcuts;
        let mut bindings = HashSet::new();
        for (action, key) in &self.shortcuts {
            let mut parts: Vec<_> = key.split('+').map(str::to_ascii_lowercase).collect();
            let last = parts.pop().unwrap_or_default();
            if !actions.contains_key(action)
                || last.is_empty()
                || parts.is_empty()
                || parts
                    .iter()
                    .any(|p| !["ctrl", "shift", "alt", "meta"].contains(&p.as_str()))
                || parts.iter().collect::<HashSet<_>>().len() != parts.len()
            {
                return Err(CoreError::new(
                    "invalid_shortcut",
                    "快捷键动作或组合无效（加号键请配置 Ctrl+=）",
                ));
            }
            parts.sort();
            parts.push(last);
            if !bindings.insert(parts.join("+")) {
                return Err(CoreError::new("shortcut_conflict", "快捷键组合冲突"));
            }
        }
        Ok(())
    }
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > 256 * 1024 {
            return Err(CoreError::new("config_size", "配置文件超过 256 KiB"));
        }
        // Inspect the version before decoding v1 fields: a newer document may also
        // contain unknown fields or change their types, but must still be read-only.
        #[derive(Deserialize)]
        struct Schema {
            schema_version: Option<toml::Value>,
        }
        let schema: Schema = toml::from_str(text)
            .map_err(|e| CoreError::new("config_parse", format!("配置格式错误：{e}")))?;
        if schema
            .schema_version
            .is_some_and(|version| version.as_integer() != Some(1))
        {
            return Err(CoreError::new(
                "schema_version",
                "配置版本不受支持；原文件保留且禁止写回",
            ));
        }
        let c: Self = toml::from_str(text)
            .map_err(|e| CoreError::new("config_parse", format!("配置格式错误：{e}")))?;
        c.validate()?;
        Ok(c)
    }
}
pub fn default_text() -> String {
    let mut text = String::from(
        "# Terminder v1\n# Shell 与目录修改只影响新会话；恢复只重建启动环境，不恢复命令。\n\n",
    );
    text.push_str(&toml::to_string_pretty(&Config::default()).expect("default serializable"));
    text
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedTab {
    pub id: String,
    pub profile_id: String,
    pub startup_cwd: Option<String>,
    pub title: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub schema_version: u32,
    pub geometry: Option<Geometry>,
    pub tabs: Vec<SavedTab>,
    pub active_tab: Option<String>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            schema_version: 1,
            geometry: None,
            tabs: vec![],
            active_tab: None,
        }
    }
}
impl Workspace {
    pub fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        if self.schema_version != 1
            || self.tabs.len() > 16
            || self.geometry.as_ref().is_some_and(|g| {
                g.width < 480 || g.width > 16000 || g.height < 320 || g.height > 16000
            })
        {
            return Err(CoreError::new(
                "workspace_invalid",
                "工作区版本、尺寸或数量无效",
            ));
        }
        for tab in &self.tabs {
            if tab.id.is_empty()
                || tab.id.len() > 64
                || !ids.insert(&tab.id)
                || tab.profile_id.len() > 64
                || tab.title.as_ref().is_some_and(|s| s.chars().count() > 80)
                || tab
                    .startup_cwd
                    .as_ref()
                    .is_some_and(|s| s.len() > 32768 || s.contains('\0'))
            {
                return Err(CoreError::new("workspace_invalid", "工作区标签信息无效"));
            }
        }
        if self.active_tab.as_ref().is_some_and(|id| !ids.contains(id)) {
            return Err(CoreError::new("workspace_invalid", "活动标签不在快照中"));
        }
        Ok(())
    }
}
