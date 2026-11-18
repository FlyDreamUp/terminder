use super::{
    input::{Input, InputEvent},
    pane::{Pane, PaneEvent},
};
use crate::{
    bridge::{Bridge, Runtime},
    config::{
        model::{
            Config, Geometry, SavedPane, SavedTab, TerminalConfig, Workspace as SavedWorkspace,
        },
        store::{self, ConfigDocument, Paths},
    },
    layout::{Axis, Direction, Layout, PaneId},
};
use gpui::{prelude::*, *};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use terminder_session_core::{model::SpawnSpec, profile};

#[derive(Clone, PartialEq, Action)]
#[action(namespace = terminder, no_json)]
pub struct Command {
    pub name: String,
}
#[derive(Clone, Copy)]
pub(super) enum CloseTarget {
    Window,
    Tab(usize),
    Pane(PaneId),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProfileMenu {
    TabBar,
    Welcome,
}
pub(super) enum Modal {
    Close(CloseTarget),
    Restore,
    Settings,
    Rename(Entity<Input>),
}
pub(super) struct Tab {
    pub title: Option<String>,
    pub layout: Layout,
    pub active: PaneId,
}
pub(super) struct Entry {
    pub view: Entity<Pane>,
    pub spec: SpawnSpec,
    pub _subscription: Subscription,
}
pub(super) struct Drag {
    pub path: Vec<bool>,
    pub axis: Axis,
    pub bounds: Bounds<Pixels>,
}
pub(super) struct Settings {
    pub terminal: TerminalConfig,
    pub font: Entity<Input>,
}
pub struct Workspace {
    pub(super) runtime: Runtime,
    pub(super) paths: Paths,
    pub(super) document: ConfigDocument,
    pub(super) tabs: Vec<Tab>,
    pub(super) panes: BTreeMap<PaneId, Entry>,
    pub(super) active: usize,
    next_id: PaneId,
    pub(super) focus: FocusHandle,
    pub(super) modal: Option<Modal>,
    pub(super) profile_menu: Option<ProfileMenu>,
    pub(super) profile_selection: usize,
    pub(super) profile_scroll: ScrollHandle,
    pub(super) search: Option<Entity<Input>>,
    pub(super) settings: Option<Settings>,
    pub(super) status: String,
    pub(super) quitting: bool,
    pub(super) config_busy: bool,
    deferred_close: Option<CloseTarget>,
    pub(super) dragging: Option<Drag>,
    pub(super) split_bounds: BTreeMap<Vec<bool>, Bounds<Pixels>>,
    #[cfg(feature = "test-support")]
    pub(super) caption_bounds: BTreeMap<&'static str, Bounds<Pixels>>,
    #[cfg(feature = "test-support")]
    pub(super) welcome_bounds: BTreeMap<&'static str, Bounds<Pixels>>,
    #[cfg(feature = "test-support")]
    pub(super) profile_bounds: BTreeMap<String, Bounds<Pixels>>,
    pub(super) light: bool,
    zoom: f32,
    requested: Option<String>,
    cwd: Option<PathBuf>,
    saved: SavedWorkspace,
    state_revision: String,
    state_writable: bool,
    _subscriptions: Vec<Subscription>,
    _input_subscription: Option<Subscription>,
    _shutdown: Option<Task<()>>,
    _config_task: Option<Task<()>>,
}
pub struct Bootstrap {
    pub paths: Paths,
    pub document: ConfigDocument,
    pub saved: SavedWorkspace,
    pub state_revision: String,
    pub state_error: Option<String>,
    pub requested: Option<String>,
    pub cwd: Option<PathBuf>,
}
pub fn bind_shortcuts(config: &Config, cx: &mut App) {
    cx.clear_key_bindings();
    cx.bind_keys(config.shortcuts.iter().map(|(name, key)| {
        KeyBinding::new(key, Command { name: name.clone() }, Some("Workspace"))
    }));
    cx.bind_keys([
        KeyBinding::new("tab", super::pane::SendTab, Some("Terminal")),
        KeyBinding::new("shift-tab", super::pane::SendBacktab, Some("Terminal")),
    ]);
}
pub fn is_light(config: &TerminalConfig, window: &Window) -> bool {
    config.theme == "light"
        || (config.theme == "system"
            && matches!(
                window.appearance(),
                WindowAppearance::Light | WindowAppearance::VibrantLight
            ))
}
impl Workspace {
    pub fn new(
        runtime: Runtime,
        boot: Bootstrap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let status = boot
            .document
            .error
            .as_ref()
            .map(ToString::to_string)
            .or(boot.state_error.clone())
            .unwrap_or_default();
        let light = is_light(&boot.document.config.terminal, window);
        let mut this = Self {
            runtime,
            paths: boot.paths,
            document: boot.document,
            tabs: vec![],
            panes: BTreeMap::new(),
            active: 0,
            next_id: 1,
            focus: cx.focus_handle(),
            modal: None,
            profile_menu: None,
            profile_selection: 0,
            profile_scroll: ScrollHandle::new(),
            search: None,
            settings: None,
            status,
            quitting: false,
            config_busy: false,
            deferred_close: None,
            dragging: None,
            split_bounds: BTreeMap::new(),
            #[cfg(feature = "test-support")]
            caption_bounds: BTreeMap::new(),
            #[cfg(feature = "test-support")]
            welcome_bounds: BTreeMap::new(),
            #[cfg(feature = "test-support")]
            profile_bounds: BTreeMap::new(),
            light,
            zoom: 0.0,
            requested: boot.requested,
            cwd: boot.cwd,
            saved: boot.saved,
            state_revision: boot.state_revision,
            state_writable: boot.state_error.is_none(),
            _subscriptions: vec![],
            _input_subscription: None,
            _shutdown: None,
            _config_task: None,
        };
        this._subscriptions
            .push(cx.observe_window_appearance(window, |this, window, cx| {
                this.refresh_appearance(window, cx)
            }));
        if this.document.config.window.restore_tabs
            && !this.saved.tabs.is_empty()
            && this.requested.is_none()
            && this.cwd.is_none()
        {
            this.modal = Some(Modal::Restore);
            this.focus.focus(window);
        } else if this.requested.is_some() || this.cwd.is_some() {
            this.new_tab(None, window, cx);
        } else {
            this.focus.focus(window);
        }
        this
    }
    fn terminal_settings(&self) -> TerminalConfig {
        let mut terminal = self.document.config.terminal.clone();
        terminal.font_size_px = (terminal.font_size_px + self.zoom).clamp(8.0, 48.0);
        terminal
    }
    fn add_pane(
        &mut self,
        id: PaneId,
        spec: SpawnSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.panes.len() < self.document.config.session.max_sessions,
            "已达到会话上限"
        );
        let bridge = Bridge::new(self.runtime.clone(), spec.clone())?;
        let terminal = self.terminal_settings();
        let clipboard = self.document.config.clipboard.clone();
        let light = self.light;
        let view = cx.new(|cx| {
            Pane::new(
                bridge,
                spec.size,
                terminal.clone(),
                clipboard.clone(),
                light,
                window,
                cx,
            )
        });
        let reserved = self
            .document
            .config
            .shortcuts
            .iter()
            .filter_map(|(_, key)| Keystroke::parse(key).ok())
            .collect();
        view.update(cx, |pane, cx| {
            pane.configure(terminal, clipboard, light, reserved, cx)
        });
        let subscription = cx.subscribe(&view, move |this, _pane, event, cx| {
            if matches!(event, PaneEvent::Focused) {
                for (index, tab) in this.tabs.iter_mut().enumerate() {
                    if tab.layout.ids().contains(&id) {
                        tab.active = id;
                        this.active = index;
                        break;
                    }
                }
            }
            cx.notify();
        });
        self.panes.insert(
            id,
            Entry {
                view,
                spec,
                _subscription: subscription,
            },
        );
        self.next_id = self.next_id.max(id + 1);
        Ok(())
    }
    fn spec(
        &self,
        requested: &str,
        inherited: Option<&std::path::Path>,
    ) -> anyhow::Result<SpawnSpec> {
        let (profile, program) =
            profile::resolve_profile(&self.document.config.profiles, requested)?;
        Ok(profile::spawn_spec(
            profile,
            program,
            self.cwd.as_deref(),
            inherited,
        )?)
    }
    pub(super) fn active_id(&self) -> Option<PaneId> {
        self.tabs.get(self.active).map(|tab| tab.active)
    }
    pub(super) fn active_view(&self) -> Option<Entity<Pane>> {
        self.active_id()
            .and_then(|id| self.panes.get(&id).map(|entry| entry.view.clone()))
    }
    pub(super) fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_view() {
            pane.read(cx).focus.focus(window);
        } else {
            self.focus.focus(window);
        }
    }
    pub(super) fn select_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.layout.ids().contains(&id))
        {
            self.active = index;
            self.tabs[index].active = id;
            self.focus_active(window, cx);
            cx.notify();
        }
    }
    pub(super) fn pane_command(
        &mut self,
        id: PaneId,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_pane(id, window, cx);
        self.command(&Command { name: name.into() }, window, cx);
    }
    pub(super) fn toggle_profiles(
        &mut self,
        origin: ProfileMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.profile_menu == Some(origin) {
            self.dismiss_profiles(window, cx);
            return;
        }
        let current = self
            .active_id()
            .and_then(|id| self.panes.get(&id))
            .map(|e| e.spec.profile_id.as_str());
        self.profile_selection = self
            .document
            .config
            .profiles
            .iter()
            .position(|p| Some(p.id.as_str()) == current)
            .unwrap_or(0);
        self.profile_scroll.scroll_to_item(self.profile_selection);
        self.profile_menu = Some(origin);
        self.focus.focus(window);
        cx.notify();
    }
    pub(super) fn dismiss_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_menu = None;
        self.focus_active(window, cx);
        cx.notify();
    }
    pub(super) fn profile_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.document.config.profiles.len();
        match event.keystroke.key.as_str() {
            "escape" => self.dismiss_profiles(window, cx),
            "enter" => {
                if let Some(profile) = self.document.config.profiles.get(self.profile_selection) {
                    self.new_tab(Some(profile.id.clone()), window, cx);
                }
            }
            "down" if count > 0 => self.profile_selection = (self.profile_selection + 1) % count,
            "up" if count > 0 => {
                self.profile_selection = (self.profile_selection + count - 1) % count
            }
            "home" => self.profile_selection = 0,
            "end" => self.profile_selection = count.saturating_sub(1),
            "tab" => self.dismiss_profiles(window, cx),
            _ => {}
        }
        self.profile_scroll.scroll_to_item(self.profile_selection);
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn new_tab(
        &mut self,
        requested: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = (|| -> anyhow::Result<()> {
            anyhow::ensure!(self.tabs.len() < 16, "标签数量已达上限");
            let requested = requested
                .or(self.requested.clone())
                .unwrap_or_else(|| self.document.config.default_profile.clone());
            let inherited = self
                .active_id()
                .and_then(|id| self.panes.get(&id))
                .map(|entry| entry.spec.cwd.as_path());
            let spec = self.spec(&requested, inherited)?;
            let id = self.next_id;
            self.add_pane(id, spec, window, cx)?;
            self.tabs.push(Tab {
                title: None,
                layout: Layout::pane(id),
                active: id,
            });
            self.active = self.tabs.len() - 1;
            self.profile_menu = None;
            self.focus_active(window, cx);
            Ok(())
        })();
        if let Err(error) = result {
            self.status = error.to_string();
        }
        cx.notify();
    }
    pub(super) fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let result = (|| -> anyhow::Result<()> {
            let id = self
                .active_id()
                .ok_or_else(|| anyhow::anyhow!("请先打开终端"))?;
            let tab = &self.tabs[self.active];
            anyhow::ensure!(
                tab.layout.ids().len() < crate::layout::MAX_PANES,
                "每个标签最多 8 个窗格"
            );
            let source = &self.panes[&id];
            let pane = source.view.read(cx);
            let bounds = pane.bounds.size;
            let needed = match axis {
                Axis::Horizontal => {
                    bounds.width >= px(pane.settings.font_size_px * 0.62 * 80.0 + 32.0)
                }
                Axis::Vertical => {
                    bounds.height >= px(pane.settings.font_size_px * 1.5 * 20.0 + 64.0)
                }
            };
            anyhow::ensure!(needed, "窗口空间不足，请扩大窗口或减小字号后分屏");
            let (profile, program) =
                profile::resolve_profile(&self.document.config.profiles, &source.spec.profile_id)?;
            let spec = profile::spawn_spec(profile, program, None, Some(&source.spec.cwd))?;
            let next = self.next_id;
            self.add_pane(next, spec, window, cx)?;
            self.tabs[self.active].layout.split(id, next, axis);
            self.tabs[self.active].active = next;
            self.focus_active(window, cx);
            Ok(())
        })();
        if let Err(error) = result {
            self.status = error.to_string();
        }
        cx.notify();
    }
    pub(super) fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active = index;
            self.split_bounds.clear();
            self.focus_active(window, cx);
            cx.notify();
        }
    }
    pub(super) fn request_close(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quitting {
            return;
        }
        self.profile_menu = None;
        if self.config_busy {
            self.deferred_close = Some(target);
            self.status = "正在保存设置，完成后继续关闭…".into();
            cx.notify();
            return;
        }
        let running = match target {
            CloseTarget::Window => self.panes.values().any(|e| e.view.read(cx).is_running()),
            CloseTarget::Tab(index) => self.tabs.get(index).is_some_and(|tab| {
                tab.layout
                    .ids()
                    .iter()
                    .any(|id| self.panes[id].view.read(cx).is_running())
            }),
            CloseTarget::Pane(id) => self
                .panes
                .get(&id)
                .is_some_and(|entry| entry.view.read(cx).is_running()),
        };
        if self.document.config.session.confirm_close_running && running {
            self.modal = Some(Modal::Close(target));
            self.focus.focus(window);
            cx.notify();
        } else {
            self.close_confirmed(target, window, cx);
        }
    }
    pub(super) fn close_confirmed(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modal = None;
        match target {
            CloseTarget::Window => {
                self.shutdown(window, cx);
                return;
            }
            CloseTarget::Tab(index) => {
                if index >= self.tabs.len() {
                    return;
                }
                let tab = self.tabs.remove(index);
                for id in tab.layout.ids() {
                    self.panes.remove(&id);
                }
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
            }
            CloseTarget::Pane(id) => {
                let Some(index) = self
                    .tabs
                    .iter()
                    .position(|tab| tab.layout.ids().contains(&id))
                else {
                    return;
                };
                if self.tabs[index].layout.ids().len() == 1 {
                    self.close_confirmed(CloseTarget::Tab(index), window, cx);
                    return;
                }
                self.panes.remove(&id);
                let tab = &mut self.tabs[index];
                tab.layout = tab.layout.clone().remove(id).expect("another pane remains");
                if tab.active == id {
                    tab.active = tab.layout.ids()[0];
                }
            }
        }
        self.search = None;
        self.dragging = None;
        self.split_bounds.clear();
        self.focus_active(window, cx);
        cx.notify();
    }
    fn snapshot(&self, window: &Window) -> SavedWorkspace {
        let bounds = window.window_bounds().get_bounds();
        SavedWorkspace {
            schema_version: 2,
            geometry: Some(Geometry {
                x: f32::from(bounds.origin.x) as i32,
                y: f32::from(bounds.origin.y) as i32,
                width: f32::from(bounds.size.width).max(600.0) as u32,
                height: f32::from(bounds.size.height).max(400.0) as u32,
                maximized: window.is_maximized(),
            }),
            active_tab: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    title: tab.title.clone(),
                    layout: tab.layout.clone(),
                    active: tab.active,
                    panes: tab
                        .layout
                        .ids()
                        .into_iter()
                        .filter_map(|id| {
                            self.panes.get(&id).map(|entry| SavedPane {
                                id,
                                profile_id: entry.spec.profile_id.clone(),
                                startup_cwd: Some(entry.spec.cwd.clone()),
                            })
                        })
                        .collect(),
                })
                .collect(),
        }
    }
    fn shutdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        self.status = "正在结束会话并保存工作区…".into();
        let snapshot = self.snapshot(window);
        let path = self.paths.state.clone();
        let expected = self.state_revision.clone();
        let writable = self.state_writable;
        let runtime = self.runtime.clone();
        runtime.begin_shutdown();
        let save = cx.background_executor().spawn(async move {
            if writable {
                store::save_workspace(&path, &snapshot, &expected).map(|_| ())
            } else {
                Err(terminder_session_core::model::CoreError::new(
                    "workspace_readonly",
                    "损坏的工作区文件已保留",
                ))
            }
        });
        let window_handle = window.window_handle();
        self._shutdown = Some(cx.spawn(async move |this, cx| {
            let saved = save.await;
            if let Err(error) = saved {
                tracing::error!(%error,"workspace save failed");
                let _ = this.update(cx, |this, cx| {
                    this.status = format!("工作区未保存：{error}");
                    cx.notify();
                });
            }
            loop {
                match runtime.poll_shutdown() {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::error!(%error,"shutdown error");
                        let _ = this.update(cx, |this, cx| {
                            this.status = error.to_string();
                            cx.notify();
                        });
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
            }
            let _ = window_handle.update(cx, |_, window, _| window.remove_window());
        }));
        cx.notify();
    }
    pub(super) fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = None;
        let saved = self.saved.clone();
        let mut errors = Vec::new();
        for tab in saved.tabs {
            let mut layout = Some(tab.layout.clone());
            for pane in tab.panes {
                let result = (|| -> anyhow::Result<()> {
                    let (profile, program) =
                        profile::resolve_profile(&self.document.config.profiles, &pane.profile_id)?;
                    let spec =
                        profile::spawn_spec(profile, program, pane.startup_cwd.as_deref(), None)?;
                    self.add_pane(pane.id, spec, window, cx)
                })();
                if let Err(error) = result {
                    errors.push(error.to_string());
                    layout = layout.and_then(|l| l.remove(pane.id));
                }
            }
            if let Some(layout) = layout {
                let active = if layout.ids().contains(&tab.active) {
                    tab.active
                } else {
                    layout.ids()[0]
                };
                self.tabs.push(Tab {
                    title: tab.title,
                    layout,
                    active,
                });
            }
        }
        self.active = saved.active_tab.min(self.tabs.len().saturating_sub(1));
        self.focus_active(window, cx);
        if !errors.is_empty() {
            self.status = format!("部分启动项无法恢复：{}", errors.join("；"));
        }
        cx.notify();
    }
    pub(super) fn cancel_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = None;
        self.settings = None;
        self.focus_active(window, cx);
        cx.notify();
    }
    pub(super) fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_menu = None;
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let input = cx.new(|cx| {
            Input::new(
                tab.title.clone().unwrap_or_default(),
                "标签名称（留空使用终端标题）",
                self.light,
                cx,
            )
        });
        self._input_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::Submit => {
                    let text = input.read(cx).text().chars().take(80).collect::<String>();
                    if let Some(tab) = this.tabs.get_mut(this.active) {
                        tab.title = (!text.is_empty()).then_some(text);
                    }
                    this.cancel_modal(window, cx);
                }
                InputEvent::Cancel => this.cancel_modal(window, cx),
            },
        ));
        input.read(cx).focus.focus(window);
        self.modal = Some(Modal::Rename(input));
        cx.notify();
    }
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_menu = None;
        if self.search.is_none() {
            let input =
                cx.new(|cx| Input::new(String::new(), "搜索历史（正则表达式）", self.light, cx));
            self._input_subscription =
                Some(
                    cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                        InputEvent::Submit => this.find(false, cx),
                        InputEvent::Cancel => this.close_search(window, cx),
                    }),
                );
            self.search = Some(input);
        }
        if let Some(input) = &self.search {
            input.read(cx).focus.focus(window);
        }
        cx.notify();
    }
    pub(super) fn find(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if let (Some(input), Some(pane)) = (&self.search, self.active_view()) {
            let query = input.read(cx).text().to_string();
            pane.update(cx, |pane, cx| pane.find(&query, backwards, cx));
        }
    }
    pub(super) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        if let Some(pane) = self.active_view() {
            pane.update(cx, |pane, cx| pane.clear_search(cx));
        }
        self.focus_active(window, cx);
        cx.notify();
    }
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_menu = None;
        let terminal = self.document.config.terminal.clone();
        let font = cx.new(|cx| {
            Input::new(
                terminal.font_family.join(", "),
                "字体名称，逗号分隔",
                self.light,
                cx,
            )
        });
        self.settings = Some(Settings { terminal, font });
        self.modal = Some(Modal::Settings);
        self.focus.focus(window);
        cx.notify();
    }
    pub(super) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_busy {
            return;
        }
        let Some(settings) = &self.settings else {
            return;
        };
        let mut terminal = settings.terminal.clone();
        terminal.font_family = settings
            .font
            .read(cx)
            .text()
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        let mut config = self.document.config.clone();
        config.terminal = terminal.clone();
        if let Err(error) = config.validate() {
            self.status = error.to_string();
            cx.notify();
            return;
        }
        self.config_busy = true;
        let path = self.paths.config.clone();
        let revision = self.document.revision.clone();
        let handle = window.window_handle();
        let work = cx
            .background_executor()
            .spawn(async move { store::update_terminal(&path, terminal, &revision) });
        self._config_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.config_busy = false;
                    match result {
                        Ok(doc) => {
                            this.document = doc;
                            this.modal = None;
                            this.settings = None;
                            this.status = "外观设置已保存".into();
                            this.refresh_appearance(window, cx);
                            this.focus_active(window, cx);
                        }
                        Err(error) => {
                            this.status = error.to_string();
                        }
                    }
                    if let Some(target) = this.deferred_close.take() {
                        this.request_close(target, window, cx);
                    }
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }
    pub(super) fn edit_config(&mut self, cx: &mut Context<Self>) {
        let result = (|| -> anyhow::Result<()> {
            if !self.paths.config.exists() {
                self.document = store::save_config(
                    &self.paths.config,
                    &self.document.text,
                    &self.document.revision,
                )?;
            }
            let editor = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("找不到 Windows 目录"))?
                .join("System32")
                .join("notepad.exe");
            std::process::Command::new(editor)
                .arg(&self.paths.config)
                .spawn()?;
            Ok(())
        })();
        self.status = match result {
            Ok(()) => "保存配置文件后点击“重新加载”。Shell 与恢复设置影响后续启动。".into(),
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_busy {
            return;
        }
        let document = store::load_config(&self.paths.config);
        if let Some(error) = &document.error {
            self.status = error.to_string();
            cx.notify();
            return;
        }
        bind_shortcuts(&document.config, cx);
        self.document = document;
        self.modal = None;
        self.settings = None;
        self.status = "配置已重新加载".into();
        self.refresh_appearance(window, cx);
        self.focus_active(window, cx);
        cx.notify();
    }
    fn refresh_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.light = is_light(&self.document.config.terminal, window);
        let terminal = self.terminal_settings();
        let clipboard = self.document.config.clipboard.clone();
        let reserved: Vec<_> = self
            .document
            .config
            .shortcuts
            .iter()
            .filter_map(|(_, key)| Keystroke::parse(key).ok())
            .collect();
        for entry in self.panes.values() {
            entry.view.update(cx, |pane, cx| {
                pane.configure(
                    terminal.clone(),
                    clipboard.clone(),
                    self.light,
                    reserved.clone(),
                    cx,
                )
            });
        }
        for input in self
            .search
            .iter()
            .chain(self.settings.iter().map(|s| &s.font))
        {
            input.update(cx, |input, cx| {
                input.light = self.light;
                cx.notify();
            });
        }
        if let Some(Modal::Rename(input)) = &self.modal {
            input.update(cx, |input, cx| {
                input.light = self.light;
                cx.notify();
            });
        }
        cx.notify();
    }
    pub(super) fn command(
        &mut self,
        action: &Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quitting || self.modal.is_some() {
            return;
        }
        match action.name.as_str() {
            "new_tab" => self.new_tab(None, window, cx),
            "close_tab" => self.request_close(CloseTarget::Tab(self.active), window, cx),
            "close_pane" => {
                if let Some(id) = self.active_id() {
                    self.request_close(CloseTarget::Pane(id), window, cx);
                }
            }
            "split_right" => self.split(Axis::Horizontal, window, cx),
            "split_down" => self.split(Axis::Vertical, window, cx),
            "next_tab" => {
                if !self.tabs.is_empty() {
                    self.select_tab((self.active + 1) % self.tabs.len(), window, cx);
                }
            }
            "previous_tab" => {
                if !self.tabs.is_empty() {
                    self.select_tab(
                        (self.active + self.tabs.len() - 1) % self.tabs.len(),
                        window,
                        cx,
                    );
                }
            }
            "focus_left" | "focus_right" | "focus_up" | "focus_down" => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    let direction = match action.name.as_str() {
                        "focus_left" => Direction::Left,
                        "focus_right" => Direction::Right,
                        "focus_up" => Direction::Up,
                        _ => Direction::Down,
                    };
                    if let Some(id) = tab.layout.neighbor(tab.active, direction) {
                        tab.active = id;
                        self.focus_active(window, cx);
                        cx.notify();
                    }
                }
            }
            "copy" => {
                if let Some(pane) = self.active_view() {
                    pane.update(cx, |pane, cx| pane.copy(window, cx));
                }
            }
            "paste" => {
                if let Some(pane) = self.active_view() {
                    pane.update(cx, |pane, cx| pane.paste(cx));
                }
            }
            "find" => self.open_search(window, cx),
            "settings" => self.open_settings(window, cx),
            "rename_tab" => self.rename(window, cx),
            "restart" => self.restart(window, cx),
            "zoom_in" | "zoom_out" | "zoom_reset" => {
                self.zoom = match action.name.as_str() {
                    "zoom_in" => self.zoom + 1.0,
                    "zoom_out" => self.zoom - 1.0,
                    _ => 0.0,
                };
                self.zoom = self.zoom.clamp(
                    8.0 - self.document.config.terminal.font_size_px,
                    48.0 - self.document.config.terminal.font_size_px,
                );
                self.refresh_appearance(window, cx);
            }
            _ => {}
        }
    }
    pub(super) fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active_id() else {
            return;
        };
        let entry = &self.panes[&id];
        if entry.view.read(cx).is_running() {
            self.status = "会话仍在运行".into();
            cx.notify();
            return;
        }
        let spec = entry.spec.clone();
        // Keep the old screen if launch allocation fails.
        let old = self.panes.remove(&id).expect("active pane");
        if let Err(error) = self.add_pane(id, spec, window, cx) {
            self.panes.insert(id, old);
            self.status = error.to_string();
        }
        self.focus_active(window, cx);
        cx.notify();
    }
}
