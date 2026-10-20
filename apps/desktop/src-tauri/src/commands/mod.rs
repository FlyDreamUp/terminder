use crate::{
    app::AppState,
    config::{model::*, store},
    platform,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tauri::{
    State, WebviewWindow,
    ipc::{Channel, InvokeBody, Request, Response},
};
use terminder_session_core::{
    model::*,
    profile::{resolve_profile, resolve_program, spawn_spec},
    supervisor::Sink,
};

fn authorize_window(window: &WebviewWindow, state: &AppState) -> Result<String> {
    if window.label() != "main" {
        return Err(CoreError::new("window_denied", "此窗口没有终端权限"));
    }
    Ok(state.window_owner())
}
fn authorize(window: &WebviewWindow, state: &AppState) -> Result<String> {
    let owner = authorize_window(window, state)?;
    state.rate_limit()?;
    Ok(owner)
}
struct CommandPermit(Arc<AtomicUsize>);
impl CommandPermit {
    fn acquire(active: &Arc<AtomicUsize>) -> Result<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 64).then_some(count + 1)
            })
            .map_err(|_| CoreError::new("control_busy", "宿主正在处理其他请求，请稍后重试"))?;
        Ok(Self(active.clone()))
    }
}
impl Drop for CommandPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
async fn blocking<T: Send + 'static>(
    state: &AppState,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    // Cap queued AND running jobs before submitting to Tauri's shared runtime. Closing bypasses it.
    let permit = CommandPermit::acquire(&state.active_commands)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|_| CoreError::new("host_worker", "宿主任务异常退出"))?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStatus {
    id: String,
    available: bool,
    error: Option<CoreError>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    document: store::ConfigDocument,
    profiles: Vec<ProfileStatus>,
    workspace: Workspace,
    workspace_error: Option<CoreError>,
    version: &'static str,
    platform: &'static str,
}
#[tauri::command]
pub async fn bootstrap(window: WebviewWindow, state: State<'_, AppState>) -> Result<Bootstrap> {
    authorize(&window, &state)?;
    let paths = state.paths.clone();
    blocking(&state, move || {
        let document = store::load_config(&paths.config);
        let profiles = document
            .config
            .profiles
            .iter()
            .map(|p| {
                let result = resolve_program(p);
                ProfileStatus {
                    id: p.id.clone(),
                    available: result.is_ok(),
                    error: result.err(),
                }
            })
            .collect();
        let (workspace, workspace_error) = match store::load_workspace(&paths.state) {
            Ok(w) => (w, None),
            Err(e) => (Workspace::default(), Some(e)),
        };
        Ok(Bootstrap {
            document,
            profiles,
            workspace,
            workspace_error,
            version: env!("CARGO_PKG_VERSION"),
            platform: std::env::consts::OS,
        })
    })
    .await
}
#[tauri::command]
pub async fn prepare_session(
    window: WebviewWindow,
    state: State<'_, AppState>,
    profile_id: String,
    cwd: Option<String>,
    inherit_session: Option<String>,
) -> Result<Binding> {
    let owner = authorize(&window, &state)?;
    if profile_id.len() > 64
        || cwd
            .as_ref()
            .is_some_and(|c| c.len() > 32768 || c.contains('\0'))
    {
        return Err(CoreError::new("invalid_request", "Profile 或目录长度无效"));
    }
    let config = state.config.lock().unwrap().clone();
    let supervisor = state.supervisor.clone();
    blocking(&state, move || {
        let inherited = if let Some(id) = inherit_session {
            supervisor
                .list(&owner)?
                .into_iter()
                .find(|s| s.binding.session_id == id)
                .and_then(|s| s.startup_cwd)
                .map(PathBuf::from)
        } else {
            None
        };
        let requested = if profile_id == "default" {
            config.default_profile.as_str()
        } else {
            &profile_id
        };
        let (profile, program) = resolve_profile(&config.profiles, requested)?;
        let explicit = cwd.map(PathBuf::from);
        let spec = spawn_spec(profile, program, explicit.as_deref(), inherited.as_deref())?;
        Ok(supervisor.prepare(&owner, spec)?.binding)
    })
    .await
}
struct Channels {
    output: Channel<Response>,
    events: Channel<SessionEvent>,
}
impl Sink for Channels {
    fn output(&self, bytes: Vec<u8>) -> Result<()> {
        self.output
            .send(Response::new(bytes))
            .map_err(|_| CoreError::new("channel_failed", "终端输出通道已失效"))
    }
    fn event(&self, event: SessionEvent) -> Result<()> {
        self.events
            .send(event)
            .map_err(|_| CoreError::new("channel_failed", "会话状态通道已失效"))
    }
}
#[tauri::command]
pub async fn start_session(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
    size: TerminalSize,
    output: Channel<Response>,
    events: Channel<SessionEvent>,
) -> Result<()> {
    let owner = authorize(&window, &state)?;
    let s = state.supervisor.clone();
    blocking(&state, move || {
        s.start(&owner, binding, size, Arc::new(Channels { output, events }))
    })
    .await
}
#[tauri::command]
pub async fn input(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> Result<()> {
    let owner = authorize(&window, &state)?;
    let header = |name: &str| -> Result<String> {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|s| s.len() <= 64)
            .map(str::to_owned)
            .ok_or_else(|| CoreError::new("input_header", "缺少有效的输入绑定请求头"))
    };
    let binding = Binding {
        session_id: header("x-session-id")?,
        view_epoch: header("x-view-epoch")?,
        protocol_version: header("x-protocol-version")?
            .parse()
            .map_err(|_| CoreError::new("protocol_version", "协议版本无效"))?,
    };
    binding.validate()?;
    let sequence = parse_counter(&header("x-input-sequence")?)?;
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) if !bytes.is_empty() && bytes.len() <= INPUT_CHUNK => bytes.clone(),
        _ => {
            return Err(CoreError::new(
                "input_body",
                "输入必须是 1–16384 字节的原始请求体",
            ));
        }
    };
    let reservation = if request.headers().contains_key("x-input-reservation") {
        Some(header("x-input-reservation")?)
    } else {
        None
    };
    let s = state.supervisor.clone();
    blocking(&state, move || {
        s.input_reserved(&owner, binding, sequence, bytes, reservation)
    })
    .await
}
#[tauri::command]
pub async fn reserve_input(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
    bytes: usize,
) -> Result<String> {
    let owner = authorize(&window, &state)?;
    if bytes == 0 || bytes > state.config.lock().unwrap().clipboard.max_paste_bytes + 12 {
        return Err(CoreError::new("paste_limit", "粘贴预约超过配置限制"));
    }
    let supervisor = state.supervisor.clone();
    blocking(&state, move || {
        supervisor.reserve_input(&owner, binding, bytes)
    })
    .await
}
#[tauri::command]
pub async fn output_consumed(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
    sequence: String,
    bytes: String,
) -> Result<()> {
    let owner = authorize(&window, &state)?;
    let sequence = parse_counter(&sequence)?;
    let bytes = parse_counter(&bytes)?;
    let s = state.supervisor.clone();
    blocking(&state, move || s.ack(&owner, binding, sequence, bytes)).await
}
#[tauri::command]
pub async fn resize_session(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
    revision: String,
    size: TerminalSize,
) -> Result<()> {
    let owner = authorize(&window, &state)?;
    let revision = parse_counter(&revision)?;
    let s = state.supervisor.clone();
    blocking(&state, move || s.resize(&owner, binding, revision, size)).await
}
#[tauri::command]
pub fn close_session(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
) -> Result<()> {
    let owner = authorize_window(&window, &state)?;
    state.supervisor.close(&owner, &binding, false)
}
#[tauri::command]
pub fn view_failed(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
) -> Result<()> {
    let owner = authorize_window(&window, &state)?;
    state.supervisor.close(&owner, &binding, true)
}
#[tauri::command]
pub async fn forget_session(
    window: WebviewWindow,
    state: State<'_, AppState>,
    binding: Binding,
) -> Result<()> {
    let owner = authorize(&window, &state)?;
    let s = state.supervisor.clone();
    blocking(&state, move || s.forget(&owner, binding)).await
}
#[tauri::command]
pub async fn list_sessions(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<SessionSnapshot>> {
    let owner = authorize(&window, &state)?;
    let s = state.supervisor.clone();
    blocking(&state, move || s.list(&owner)).await
}
#[tauri::command]
pub async fn load_config(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<store::ConfigDocument> {
    authorize(&window, &state)?;
    let path = state.paths.config.clone();
    blocking(&state, move || Ok(store::load_config(&path))).await
}
#[tauri::command]
pub async fn save_config(
    window: WebviewWindow,
    state: State<'_, AppState>,
    text: String,
    revision: String,
) -> Result<store::ConfigDocument> {
    authorize(&window, &state)?;
    let path = state.paths.config.clone();
    let writer = state.writer.clone();
    let document = blocking(&state, move || {
        let _guard = writer.lock().unwrap();
        store::save_config(&path, &text, &revision)
    })
    .await?;
    state
        .supervisor
        .set_limit(document.config.session.max_sessions)?;
    *state.config.lock().unwrap() = document.config.clone();
    Ok(document)
}
#[tauri::command]
pub async fn save_terminal(
    window: WebviewWindow,
    state: State<'_, AppState>,
    terminal: TerminalConfig,
    revision: String,
) -> Result<store::ConfigDocument> {
    authorize(&window, &state)?;
    let path = state.paths.config.clone();
    let writer = state.writer.clone();
    let document = blocking(&state, move || {
        let _guard = writer.lock().unwrap();
        store::update_terminal(&path, terminal, &revision)
    })
    .await?;
    *state.config.lock().unwrap() = document.config.clone();
    Ok(document)
}
#[tauri::command]
pub async fn save_workspace(
    window: WebviewWindow,
    state: State<'_, AppState>,
    mut workspace: Workspace,
) -> Result<()> {
    let owner = authorize(&window, &state)?;
    let path = state.paths.state.clone();
    let writer = state.writer.clone();
    let s = state.supervisor.clone();
    let revision = state.workspace_revision.clone();
    // Persist startup metadata only, and require each Profile/cwd to belong to this window's sessions.
    blocking(&state, move || {
        let sessions = s.list(&owner)?;
        for tab in &workspace.tabs {
            if !sessions
                .iter()
                .any(|s| s.profile_id == tab.profile_id && s.startup_cwd == tab.startup_cwd)
            {
                return Err(CoreError::new(
                    "workspace_owner",
                    "工作区包含未归属当前窗口的会话",
                ));
            }
        }
        let _guard = writer.lock().unwrap();
        let old = store::load_workspace(&path)?;
        workspace.geometry = old.geometry;
        let mut revision = revision.lock().unwrap();
        *revision = store::save_workspace(&path, &workspace, &revision)?;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn read_clipboard(window: WebviewWindow, state: State<'_, AppState>) -> Result<String> {
    authorize(&window, &state)?;
    blocking(&state, platform::windows::read_clipboard).await
}
#[tauri::command]
pub async fn write_clipboard(
    window: WebviewWindow,
    state: State<'_, AppState>,
    text: String,
) -> Result<()> {
    authorize(&window, &state)?;
    let owner = window
        .hwnd()
        .map_err(|_| CoreError::new("window_handle", "窗口句柄不可用"))?
        .0 as isize;
    blocking(&state, move || {
        platform::windows::write_clipboard(&text, owner)
    })
    .await
}
#[tauri::command]
pub async fn open_link(
    window: WebviewWindow,
    state: State<'_, AppState>,
    url: String,
) -> Result<()> {
    authorize(&window, &state)?;
    blocking(&state, move || platform::windows::open_link(&url)).await
}
#[tauri::command]
pub async fn diagnostics(window: WebviewWindow, state: State<'_, AppState>) -> Result<String> {
    let owner = authorize(&window, &state)?;
    let s = state.supervisor.clone();
    blocking(&state, move || {
        let snapshots=s.list(&owner)?;
        let counts=snapshots.iter().map(|s|format!("{}: {:?}, congested={}, tail_complete={}, cleanup_error={}",s.binding.session_id,s.phase,s.congested,s.tail_complete,s.cleanup_error.as_ref().map(|e|e.code.as_str()).unwrap_or("none"))).collect::<Vec<_>>().join("\n");
        Ok(format!("Terminder {}\nPlatform: {} / {}\nProtocol: {}\nSessions: {}\n{}\n\nDoes not include terminal content, clipboard, paths, command arguments or environment.\n",env!("CARGO_PKG_VERSION"),std::env::consts::OS,std::env::consts::ARCH,PROTOCOL_VERSION,snapshots.len(),counts))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_budget_is_bounded_and_returns_on_failure_or_completion() {
        let active = Arc::new(AtomicUsize::new(0));
        let mut permits: Vec<_> = (0..64)
            .map(|_| CommandPermit::acquire(&active).unwrap())
            .collect();
        assert!(CommandPermit::acquire(&active).is_err());
        permits.pop();
        let last = CommandPermit::acquire(&active).unwrap();
        drop(permits);
        drop(last);
        assert_eq!(active.load(Ordering::Acquire), 0);
    }
}
