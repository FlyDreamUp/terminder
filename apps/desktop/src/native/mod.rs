mod chrome;
mod clipboard;
#[cfg(feature = "test-support")]
mod harness;
mod input;
mod pane;
mod view;
mod workspace;

use crate::{
    bridge::Runtime,
    config::{
        model::{Config, Workspace as SavedWorkspace},
        store::{self, Paths},
    },
};
use gpui::*;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use terminder_session_core::{backend, supervisor::Supervisor};
use workspace::{Bootstrap, Workspace};

pub fn run(paths: Paths, requested: Option<String>, cwd: Option<PathBuf>) -> anyhow::Result<()> {
    run_app(paths, requested, cwd, None)
}
fn run_app(
    paths: Paths,
    requested: Option<String>,
    cwd: Option<PathBuf>,
    report: Option<PathBuf>,
) -> anyhow::Result<()> {
    #[cfg(not(feature = "test-support"))]
    let _ = report;
    #[cfg(feature = "test-support")]
    if let Some(report) = &report {
        if let Some(parent) = report.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            report,
            serde_json::to_vec_pretty(
                &serde_json::json!({"passed":false,"error":"Native window check did not complete"}),
            )?,
        )?;
    }
    let document = store::load_config(&paths.config);
    let loaded = store::load_workspace(&paths.state);
    let state_error = loaded.as_ref().err().map(ToString::to_string);
    let saved = loaded.unwrap_or_default();
    let state_revision = store::file_revision(&paths.state).unwrap_or_else(|_| "unreadable".into());
    let runtime = Runtime::new(Supervisor::new(backend::native_factory(), 16)?);
    let app_runtime = runtime.clone();
    let failure = Arc::new(Mutex::new(None));
    let app_failure = failure.clone();
    Application::new().run(move |cx: &mut App| {
        workspace::bind_shortcuts(&document.config, cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = restore_bounds(&document.config, &saved, cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                window_min_size: Some(size(px(600.0), px(400.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Terminder".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                app_id: Some("Terminder".into()),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| {
                    Workspace::new(
                        app_runtime,
                        Bootstrap {
                            paths,
                            document,
                            saved,
                            state_revision,
                            state_error,
                            requested,
                            cwd,
                        },
                        window,
                        cx,
                    )
                });
                #[cfg(feature = "test-support")]
                if let Some(report) = report {
                    view.update(cx, |_, cx| harness::start(window, report, cx));
                }
                let weak = view.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    weak.update(cx, |this, cx| {
                        this.request_close(workspace::CloseTarget::Window, window, cx);
                        false
                    })
                    .unwrap_or(true)
                });
                view
            },
        );
        if let Err(error) = result {
            *app_failure.lock().unwrap() = Some(error.to_string());
            cx.quit();
        } else {
            cx.activate(true);
        }
    });
    runtime.shutdown_blocking()?;
    if let Some(error) = failure.lock().unwrap().take() {
        anyhow::bail!("无法创建 GPUI 窗口：{error}");
    }
    Ok(())
}
fn restore_bounds(config: &Config, saved: &SavedWorkspace, cx: &App) -> WindowBounds {
    let fallback = WindowBounds::centered(size(px(1200.0), px(800.0)), cx);
    let Some(geometry) = saved
        .geometry
        .as_ref()
        .filter(|_| config.window.restore_geometry)
    else {
        return fallback;
    };
    let bounds = Bounds::new(
        point(px(geometry.x as f32), px(geometry.y as f32)),
        size(px(geometry.width as f32), px(geometry.height as f32)),
    );
    // The old monitor may be disconnected. Require the titlebar to remain reachable.
    let reachable = cx.displays().iter().any(|display| {
        let screen = display.bounds();
        bounds.left() >= screen.left()
            && bounds.top() >= screen.top()
            && bounds.left() + px(160.0) <= screen.right()
            && bounds.top() + px(40.0) <= screen.bottom()
    });
    if !reachable {
        return fallback;
    }
    if geometry.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

#[cfg(feature = "test-support")]
pub fn run_smoke(fixture: PathBuf, report: PathBuf) -> anyhow::Result<()> {
    let paths = harness::paths(&fixture, &report)?;
    crate::logging::initialize(&paths.logs)?;
    run_app(paths, None, None, Some(report.clone()))?;
    let mut result: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    anyhow::ensure!(
        result["passed"] == true,
        "GPUI interface checks failed: {}",
        result["error"]
    );
    let state_path = result["state_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing state path"))?;
    let saved = store::load_workspace(std::path::Path::new(state_path))?;
    anyhow::ensure!(
        saved.tabs.len() == 2,
        "native workspace did not persist two tabs"
    );
    anyhow::ensure!(
        saved.tabs.iter().map(|t| t.panes.len()).sum::<usize>() == 3,
        "native workspace pane count mismatch"
    );
    result["shutdown_joined"] = true.into();
    result["workspace_roundtrip"] = true.into();
    std::fs::write(&report, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}

#[cfg(feature = "test-support")]
pub fn run_restore_smoke(previous: PathBuf, report: PathBuf) -> anyhow::Result<()> {
    let previous: serde_json::Value = serde_json::from_slice(&std::fs::read(previous)?)?;
    let state = PathBuf::from(
        previous["state_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing previous state path"))?,
    );
    let base = state
        .parent()
        .ok_or_else(|| anyhow::anyhow!("state parent missing"))?;
    let paths = Paths {
        config: base.join("native.toml"),
        state: state.clone(),
        logs: base.join("logs"),
    };
    crate::logging::initialize(&paths.logs)?;
    run_app(paths, None, None, Some(report.clone()))?;
    let mut result: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    if result["passed"] == true {
        let saved = store::load_workspace(&state)?;
        anyhow::ensure!(
            saved.tabs.is_empty() && saved.active_tab == 0,
            "empty workspace did not persist"
        );
        result["empty_workspace_roundtrip"] = true.into();
    }
    result["shutdown_joined"] = true.into();
    std::fs::write(&report, serde_json::to_vec_pretty(&result)?)?;
    anyhow::ensure!(
        result["passed"] == true,
        "GPUI restore checks failed: {}",
        result["error"]
    );
    Ok(())
}
