#[cfg(feature = "test-support")]
mod diagnostics;
mod logging;
mod webview;
use crate::{
    commands,
    config::{
        model::*,
        store::{self, Paths},
    },
    platform::windows,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use terminder_session_core::{
    backend::native_factory,
    model::{CoreError, Phase, Result},
    supervisor::Supervisor,
};

pub struct AppState {
    pub supervisor: Supervisor,
    pub config: Mutex<Config>,
    pub paths: Paths,
    pub writer: Arc<Mutex<()>>,
    pub workspace_revision: Arc<Mutex<String>>,
    pub window_epoch: Arc<AtomicU64>,
    pub active_commands: Arc<AtomicUsize>,
    closing: AtomicBool,
    allowed_exit: AtomicBool,
    requests: Mutex<(Instant, u32)>,
}
impl AppState {
    pub fn window_owner(&self) -> String {
        format!("main:{}", self.window_epoch.load(Ordering::Acquire))
    }
    pub fn rate_limit(&self) -> Result<()> {
        let mut rate = self.requests.lock().unwrap();
        if rate.0.elapsed() >= Duration::from_secs(1) {
            *rate = (Instant::now(), 0);
        }
        rate.1 += 1;
        if rate.1 > 4000 {
            return Err(CoreError::new("request_rate", "请求频率过高"));
        }
        Ok(())
    }
}
fn request_exit(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    if state.closing.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let app2 = app.clone();
        let accepted = tauri::async_runtime::spawn_blocking(move || {
            let state = app2.state::<AppState>();
            let config = state.config.lock().unwrap().clone();
            let count = state
                .supervisor
                .list(&state.window_owner())
                .map(|s| s.iter().filter(|s| s.phase != Phase::Finished).count())
                .unwrap_or(1);
            if count > 0
                && config.session.confirm_close_running
                && !windows::confirm(&format!(
                    "关闭窗口会结束 {count} 个会话及其受管理程序。未保存的工作可能丢失。是否继续？"
                ))
            {
                return false;
            }
            if config.window.restore_geometry {
                save_geometry(&app2);
            }
            state.supervisor.shutdown();
            true
        })
        .await
        .unwrap_or(false);
        let state = app.state::<AppState>();
        if !accepted {
            state.closing.store(false, Ordering::Release);
            return;
        }
        let started = Instant::now();
        let mut asked = false;
        loop {
            if state.supervisor.is_stopped() {
                if let Err(error) = state.supervisor.join_if_stopped() {
                    windows::show_error(&error.message);
                }
                state.allowed_exit.store(true, Ordering::Release);
                app.exit(0);
                break;
            }
            if started.elapsed() > Duration::from_secs(6) && !asked {
                asked = true;
                let force=tauri::async_runtime::spawn_blocking(||windows::confirm("部分资源尚未完成回收。选择“否”继续等待；选择“是”强制退出应用，未显示的尾部输出将丢失。是否强制退出？")).await.unwrap_or(false);
                if force {
                    std::process::exit(1);
                } // OS closes non-inherited Job handles even on this explicit emergency path.
            }
            tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(50)))
                .await
                .ok();
        }
    });
}
fn save_geometry(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    let state = app.state::<AppState>();
    let _writer = state.writer.lock().unwrap();
    if let Ok(mut workspace) = store::load_workspace(&state.paths.state) {
        workspace.geometry = Some(Geometry {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized: window.is_maximized().unwrap_or(false),
        });
        let mut revision = state.workspace_revision.lock().unwrap();
        match store::save_workspace(&state.paths.state, &workspace, &revision) {
            Ok(next) => *revision = next,
            Err(error) => tracing::warn!(code=%error.code,"geometry_save_failed"),
        }
    }
}
fn navigation_allowed(url: &url::Url) -> bool {
    let bundled = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (["http", "https"].contains(&url.scheme()) && url.host_str() == Some("tauri.localhost"));
    bundled
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(1420))
}
pub fn run() {
    let paths = match Paths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            windows::show_error(&error.message);
            return;
        }
    };
    if logging::initialize(&paths.logs).is_err() {
        windows::show_error("诊断日志目录不可写；应用可继续运行，但本次诊断无法保存。");
    }
    let document = store::load_config(&paths.config);
    let supervisor = match Supervisor::new(native_factory(), document.config.session.max_sessions) {
        Ok(s) => s,
        Err(error) => {
            windows::show_error(&error.message);
            return;
        }
    };
    let workspace_revision = Arc::new(Mutex::new(
        store::file_revision(&paths.state).unwrap_or_else(|_| "unreadable".into()),
    ));
    let state = AppState {
        window_epoch: Arc::new(AtomicU64::new(0)),
        active_commands: Arc::new(AtomicUsize::new(0)),
        supervisor,
        workspace_revision,
        config: Mutex::new(document.config),
        paths,
        writer: Arc::new(Mutex::new(())),
        closing: AtomicBool::new(false),
        allowed_exit: AtomicBool::new(false),
        requests: Mutex::new((Instant::now(), 0)),
    };
    let builder = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::prepare_session,
            commands::start_session,
            commands::input,
            commands::reserve_input,
            commands::output_consumed,
            commands::resize_session,
            commands::close_session,
            commands::view_failed,
            commands::forget_session,
            commands::list_sessions,
            commands::load_config,
            commands::save_config,
            commands::save_terminal,
            commands::save_workspace,
            commands::read_clipboard,
            commands::write_clipboard,
            commands::open_link,
            commands::diagnostics
        ])
        .setup(|app| {
            let state = app.state::<AppState>();
            let epoch = state.window_epoch.clone();
            let supervisor = state.supervisor.clone();
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Terminder")
                    .inner_size(1100.0, 720.0)
                    .min_inner_size(640.0, 400.0)
                    .decorations(true)
                    .data_directory(state.paths.logs.parent().unwrap().join("webview"))
                    .on_navigation(navigation_allowed)
                    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                    .on_page_load(move |_window, payload| {
                        if payload.event() == tauri::webview::PageLoadEvent::Started {
                            let previous = epoch.fetch_add(1, Ordering::AcqRel);
                            supervisor.view_lost(&format!("main:{previous}"));
                        }
                    });
            #[cfg(feature = "test-support")]
            let window = {
                let port = std::env::var_os("TERMINDER_TEST_CDP_PORT");
                match diagnostics::browser_arguments(
                    port.as_deref(),
                    std::env::var_os("TERMINDER_TEST_DATA_ROOT").is_some(),
                )? {
                    Some(arguments) => window.additional_browser_args(&arguments),
                    None => window,
                }
            };
            tracing::info!(
                diagnostic = cfg!(feature = "test-support"),
                "webview_starting"
            );
            let window = window.build()?;
            tracing::info!("webview_ready");
            webview::monitor(&window)?;
            if state.config.lock().unwrap().window.restore_geometry
                && let Ok(workspace) = store::load_workspace(&state.paths.state)
                && let Some(g) = workspace.geometry
            {
                let visible = window.available_monitors()?.iter().any(|m| {
                    let p = m.position();
                    let s = m.size();
                    i64::from(g.x) + 100 > i64::from(p.x)
                        && i64::from(g.x) < i64::from(p.x) + i64::from(s.width) - 100
                        && i64::from(g.y) + 50 > i64::from(p.y)
                        && i64::from(g.y) < i64::from(p.y) + i64::from(s.height) - 50
                });
                if visible {
                    window.set_size(tauri::PhysicalSize::new(g.width, g.height))?;
                    window.set_position(tauri::PhysicalPosition::new(g.x, g.y))?;
                } else {
                    window.center()?;
                }
                if g.maximized {
                    window.maximize()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                request_exit(window.app_handle().clone());
            }
            if let WindowEvent::Destroyed = event {
                window.state::<AppState>().supervisor.shutdown();
            }
        });
    match builder.build(tauri::generate_context!()) {
        Ok(app) => app.run(|handle, event| {
            if let RunEvent::ExitRequested { api, .. } = event
                && !handle
                    .state::<AppState>()
                    .allowed_exit
                    .load(Ordering::Acquire)
            {
                api.prevent_exit();
                request_exit(handle.clone());
            }
        }),
        Err(error) => {
            tracing::error!(%error, "app_start_failed");
            windows::show_error(&format!(
                "无法启动 Terminder。请确认 Microsoft Edge WebView2 Runtime 已安装且可用。\n\n{error}"
            ));
        }
    }
}
