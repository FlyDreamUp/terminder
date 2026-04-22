use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus::prelude::*;

use crate::app::AppController;
use crate::session::{SessionStatus, TabId};
use crate::ui::settings;
use crate::ui::status_bar;
use crate::ui::tab_strip;
use crate::ui::terminal_pane;

pub fn app_shell() -> Element {
    let bootstrap = crate::ui::bootstrap().clone();
    let default_font_size = bootstrap.config.terminal.font_size.to_string();
    let default_scrollback = bootstrap.config.terminal.scrollback_lines.to_string();

    let controller = use_hook(move || {
        Arc::new(Mutex::new(
            AppController::bootstrap(bootstrap.config_store, bootstrap.config, bootstrap.shell)
                .expect("failed to bootstrap app controller"),
        ))
    });

    let mut redraw_nonce = use_signal(|| 0_u64);
    let mut rename_line = use_signal(String::new);
    let mut renaming_tab_id = use_signal(|| None::<TabId>);
    let mut font_size_input = use_signal(move || default_font_size);
    let mut scrollback_input = use_signal(move || default_scrollback);

    let _ = redraw_nonce();

    {
        let controller = controller.clone();
        use_drop(move || {
            if let Ok(mut ctrl) = controller.lock() {
                ctrl.shutdown();
            }
        });
    }

    {
        let controller = controller.clone();
        let mut redraw_nonce = redraw_nonce;
        use_future(move || async move {
            loop {
                tokio::time::sleep(Duration::from_millis(16)).await;
                let mut has_changes = false;
                if let Ok(mut ctrl) = controller.lock() {
                    has_changes = ctrl.poll_events();
                }
                if has_changes {
                    let v = redraw_nonce();
                    redraw_nonce.set(v.wrapping_add(1));
                }
            }
        });
    }

    let snapshot = {
        let mut ctrl = controller.lock().expect("controller lock poisoned");
        let _ = ctrl.poll_events();

        let tabs = ctrl.state.tabs.clone();
        let active_tab = ctrl.active_tab();
        let active_output = active_tab
            .as_ref()
            .map(|tab| ctrl.terminal_output(tab.id))
            .unwrap_or_default();
        let show_settings = ctrl.state.show_settings;
        let last_error = ctrl.state.last_error.clone();
        (tabs, active_tab, active_output, show_settings, last_error)
    };

    let (tabs, active_tab, active_output, show_settings, last_error) = snapshot;
    let active_id = active_tab.as_ref().map(|tab| tab.id);

    rsx! {
        style { {APP_STYLE} }

        div { class: "app-shell",
            div { class: "toolbar",
                button {
                    onclick: {
                        let controller = controller.clone();
                        move |_| {
                            if let Ok(mut ctrl) = controller.lock() {
                                let _ = ctrl.resize_active(120, 32);
                                let _ = ctrl.poll_events();
                            }
                            let v = redraw_nonce();
                            redraw_nonce.set(v.wrapping_add(1));
                        }
                    },
                    "Refresh"
                }
                button {
                    onclick: {
                        let controller = controller.clone();
                        move |_| {
                            if let Ok(mut ctrl) = controller.lock() {
                                ctrl.state.show_settings = !ctrl.state.show_settings;
                            }
                            let v = redraw_nonce();
                            redraw_nonce.set(v.wrapping_add(1));
                        }
                    },
                    if show_settings { "Hide Settings" } else { "Settings" }
                }
                button {
                    disabled: active_id.is_none(),
                    onclick: {
                        let active_tab = active_tab.clone();
                        move |_| {
                            if let Some(tab) = active_tab.as_ref() {
                                rename_line.set(tab.title.clone());
                                renaming_tab_id.set(Some(tab.id));
                                let v = redraw_nonce();
                                redraw_nonce.set(v.wrapping_add(1));
                            }
                        }
                    },
                    "Rename"
                }
            }

            div { class: "tab-strip",
                div { class: "tab-list",
                    for tab in tabs.iter() {
                        {
                            let tab_id = tab.id;
                            let is_active = Some(tab_id) == active_id;
                            let is_renaming = renaming_tab_id() == Some(tab_id);
                            let class_name = if is_active { "tab active" } else { "tab" };
                            let label = tab_strip::tab_label(tab, is_active);
                            rsx! {
                                div {
                                    key: "{tab_id.0}",
                                    class: "{class_name}",
                                    if is_renaming {
                                        rsx! {
                                            input {
                                                class: "tab-rename-input",
                                                r#type: "text",
                                                value: "{rename_line()}",
                                                autofocus: true,
                                                oninput: move |evt| rename_line.set(evt.value()),
                                                onkeydown: {
                                                    let controller = controller.clone();
                                                    move |evt| {
                                                        match evt.key() {
                                                            Key::Enter => {
                                                                let name = rename_line().trim().to_string();
                                                                if !name.is_empty() {
                                                                    if let Ok(mut ctrl) = controller.lock() {
                                                                        ctrl.rename_tab(tab_id, name);
                                                                        let _ = ctrl.poll_events();
                                                                    }
                                                                }
                                                                renaming_tab_id.set(None);
                                                                rename_line.set(String::new());
                                                                let v = redraw_nonce();
                                                                redraw_nonce.set(v.wrapping_add(1));
                                                            }
                                                            Key::Escape => {
                                                                renaming_tab_id.set(None);
                                                                rename_line.set(String::new());
                                                                let v = redraw_nonce();
                                                                redraw_nonce.set(v.wrapping_add(1));
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                },
                                                onblur: {
                                                    let controller = controller.clone();
                                                    move |_| {
                                                        let name = rename_line().trim().to_string();
                                                        if !name.is_empty() {
                                                            if let Ok(mut ctrl) = controller.lock() {
                                                                ctrl.rename_tab(tab_id, name);
                                                                let _ = ctrl.poll_events();
                                                            }
                                                        }
                                                        renaming_tab_id.set(None);
                                                        rename_line.set(String::new());
                                                        let v = redraw_nonce();
                                                        redraw_nonce.set(v.wrapping_add(1));
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        rsx! {
                                            button {
                                                class: "tab-label",
                                                onclick: {
                                                    let controller = controller.clone();
                                                    move |_| {
                                                        if let Ok(mut ctrl) = controller.lock() {
                                                            ctrl.switch_tab(tab_id);
                                                            let _ = ctrl.poll_events();
                                                        }
                                                        let v = redraw_nonce();
                                                        redraw_nonce.set(v.wrapping_add(1));
                                                    }
                                                },
                                                "{label}"
                                            }
                                        }
                                    }
                                    button {
                                        class: "tab-close",
                                        onclick: {
                                            let controller = controller.clone();
                                            move |evt| {
                                                evt.stop_propagation();
                                                if let Ok(mut ctrl) = controller.lock() {
                                                    let _ = ctrl.close_tab(tab_id, false);
                                                    let _ = ctrl.poll_events();
                                                }
                                                let v = redraw_nonce();
                                                redraw_nonce.set(v.wrapping_add(1));
                                            }
                                        },
                                        "x"
                                    }
                                }
                            }
                        }
                    }
                }
                button {
                    class: "tab add-tab",
                    onclick: {
                        let controller = controller.clone();
                        move |_| {
                            if let Ok(mut ctrl) = controller.lock() {
                                let _ = ctrl.new_tab(None);
                                let _ = ctrl.poll_events();
                            }
                            let v = redraw_nonce();
                            redraw_nonce.set(v.wrapping_add(1));
                        }
                    },
                    "+"
                }
            }

            div { class: "terminal-pane",
                div { class: "terminal-output-wrap",
                    pre { class: "terminal-output", "{terminal_pane::format_output(&active_output)}" }
                }
                div { class: "terminal-input-inline",
                    span { class: "prompt", "{prompt}" }
                    input {
                        class: "terminal-inline-field",
                        r#type: "text",
                        value: "{input_line()}",
                        placeholder: "输入命令后按 Enter",
                        oninput: move |evt| input_line.set(evt.value()),
                        onkeydown: {
                            let controller = controller.clone();
                            move |evt| {
                                match evt.key() {
                                    Key::Enter => {
                                        let value = input_line();
                                        let trimmed = value.trim().to_string();
                                        if trimmed.is_empty() {
                                            return;
                                        }
                                        if let Ok(mut ctrl) = controller.lock() {
                                            let _ = ctrl.send_input_to_active(&trimmed);
                                            let _ = ctrl.poll_events();
                                        }
                                        let mut history = command_history();
                                        if history.last().map(|last| last != &trimmed).unwrap_or(true) {
                                            history.push(trimmed);
                                            command_history.set(history);
                                        }
                                        history_cursor.set(None);
                                        input_line.set(String::new());
                                        let v = redraw_nonce();
                                        redraw_nonce.set(v.wrapping_add(1));
                                    }
                                    Key::ArrowUp => {
                                        let history = command_history();
                                        if history.is_empty() {
                                            return;
                                        }
                                        let next = history_cursor()
                                            .map(|idx| idx.saturating_sub(1))
                                            .unwrap_or(history.len().saturating_sub(1));
                                        history_cursor.set(Some(next));
                                        if let Some(cmd) = history.get(next) {
                                            input_line.set(cmd.clone());
                                        }
                                    }
                                    Key::ArrowDown => {
                                        let history = command_history();
                                        if history.is_empty() {
                                            return;
                                        }
                                        match history_cursor() {
                                            Some(idx) if idx + 1 < history.len() => {
                                                let next = idx + 1;
                                                history_cursor.set(Some(next));
                                                if let Some(cmd) = history.get(next) {
                                                    input_line.set(cmd.clone());
                                                }
                                            }
                                            _ => {
                                                history_cursor.set(None);
                                                input_line.set(String::new());
                                            }
                                        }
                                    }
                                    Key::Escape => {
                                        history_cursor.set(None);
                                        input_line.set(String::new());
                                    }
                                    _ => {}
                                }
                            }
                        },
                    }
                }
            }

            if let Some(tab) = active_tab.as_ref() {
                div { class: "status-bar",
                    "{status_bar::build_status(tab)}"
                }
            } else {
                div { class: "status-bar", "No active tab" }
            }

            if show_settings {
                div { class: "settings-panel",
                    h3 { "Settings" }
                    div { class: "settings-row",
                        label { "Font Size" }
                        input {
                            r#type: "text",
                            value: "{font_size_input()}",
                            oninput: move |evt| font_size_input.set(evt.value()),
                        }
                    }
                    div { class: "settings-row",
                        label { "Scrollback Lines" }
                        input {
                            r#type: "text",
                            value: "{scrollback_input()}",
                            oninput: move |evt| scrollback_input.set(evt.value()),
                        }
                    }
                    button {
                        onclick: {
                            let controller = controller.clone();
                            move |_| {
                                let font_result = settings::parse_font_size(&font_size_input());
                                let scroll_result = settings::parse_scrollback(&scrollback_input());
                                if let Ok(mut ctrl) = controller.lock() {
                                    match (font_result, scroll_result) {
                                        (Ok(font_size), Ok(scrollback_lines)) => {
                                            if let Err(err) = settings::apply_terminal_settings(
                                                &mut ctrl,
                                                font_size,
                                                scrollback_lines,
                                            ) {
                                                ctrl.state.last_error = Some(err.to_string());
                                            } else {
                                                ctrl.state.last_error = None;
                                            }
                                        }
                                        (Err(err), _) | (_, Err(err)) => {
                                            ctrl.state.last_error = Some(err.to_string());
                                        }
                                    }
                                }
                                let v = redraw_nonce();
                                redraw_nonce.set(v.wrapping_add(1));
                            }
                        },
                        "Save Settings"
                    }
                }
            }

            if let Some(err) = last_error.as_ref() {
                div { class: "error-banner", "{err}" }
            }

            if let Some(tab) = active_tab.as_ref() {
                if matches!(tab.status, SessionStatus::Failed { .. }) {
                    div { class: "error-banner", "Active session failed. Open a new tab to continue." }
                }
            }
        }
    }
}

const APP_STYLE: &str = r#"
* { box-sizing: border-box; }
body { margin: 0; font-family: "Segoe UI", sans-serif; background: #121212; color: #e6e6e6; }
.app-shell { display: flex; flex-direction: column; height: 100vh; }
.toolbar { display: flex; gap: 8px; padding: 8px; border-bottom: 1px solid #2d2d2d; }
.toolbar button { background: #1f2937; color: #e6e6e6; border: 1px solid #374151; border-radius: 6px; padding: 6px 10px; cursor: pointer; }
.toolbar button:disabled { opacity: 0.45; cursor: not-allowed; }
.tab-strip { display: flex; align-items: center; gap: 8px; padding: 8px; border-bottom: 1px solid #2d2d2d; }
.tab-list { flex: 1; min-width: 0; display: flex; gap: 4px; overflow-x: auto; }
.tab { display: flex; align-items: center; gap: 6px; background: #1f2937; color: #e6e6e6; border: 1px solid #374151; border-radius: 6px; padding: 0 4px 0 8px; white-space: nowrap; }
.tab.active { background: #334155; border-color: #60a5fa; }
.tab-label { background: transparent; color: inherit; border: none; padding: 6px 0; cursor: pointer; white-space: nowrap; text-align: left; }
.tab-rename-input { min-width: 120px; background: #0f172a; color: #e6e6e6; border: 1px solid #60a5fa; border-radius: 4px; padding: 4px 6px; outline: none; font-size: 12px; }
.tab-close { width: 20px; height: 20px; background: #111827; color: #cbd5e1; border: 1px solid #374151; border-radius: 4px; cursor: pointer; line-height: 1; padding: 0; }
.tab.add-tab { flex: 0 0 auto; width: 30px; justify-content: center; padding: 6px 0; text-align: center; font-weight: 700; cursor: pointer; }
.terminal-pane { flex: 1; display: flex; flex-direction: column; background: #0b0f19; }
.terminal-output-wrap { flex: 1; overflow: auto; padding: 12px; }
.terminal-output { margin: 0; white-space: pre-wrap; word-break: break-word; font-family: "Cascadia Mono", Consolas, monospace; font-size: 13px; line-height: 1.4; }
.terminal-input-inline { display: flex; align-items: center; gap: 8px; border-top: 1px solid #2d2d2d; padding: 8px 12px; }
.terminal-inline-field { flex: 1; border: none; outline: none; background: transparent; color: #e6e6e6; font-family: "Cascadia Mono", Consolas, monospace; font-size: 13px; }
.terminal-inline-field::placeholder { color: #6b7280; }
.prompt { color: #93c5fd; white-space: nowrap; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
.settings-row input { flex: 1; background: #111827; color: #e6e6e6; border: 1px solid #374151; border-radius: 6px; padding: 6px 8px; }
.settings-panel button { background: #1f2937; color: #e6e6e6; border: 1px solid #374151; border-radius: 6px; padding: 6px 10px; cursor: pointer; }
.status-bar { padding: 8px 12px; border-top: 1px solid #2d2d2d; background: #111827; color: #b6bcc7; font-size: 12px; }
.settings-panel { border-top: 1px solid #2d2d2d; background: #0f172a; padding: 12px; display: flex; flex-direction: column; gap: 10px; }
.settings-panel h3 { margin: 0; font-size: 16px; }
.settings-row { display: flex; align-items: center; gap: 8px; }
.settings-row label { width: 130px; color: #9aa4b2; }
.error-banner { margin: 8px; padding: 8px 10px; border-radius: 6px; border: 1px solid #9f1239; background: #3f0d1f; color: #fecdd3; }
"#;
