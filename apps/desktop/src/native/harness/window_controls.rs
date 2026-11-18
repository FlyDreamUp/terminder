//! Sends mouse messages only to this isolated GPUI window, through its rendered controls.
use super::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::{
        HTCLOSE, HTMAXBUTTON, HTMINBUTTON, IsIconic, PostMessageW, SC_RESTORE, SendMessageW,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
        WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSCOMMAND,
    },
};

fn hwnd(window: &Window) -> anyhow::Result<HWND> {
    match HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!("window handle: {error}"))?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as HWND),
        _ => anyhow::bail!("expected this GPUI window's Win32 handle"),
    }
}
fn post(handle: HWND, message: u32, wparam: usize, lparam: isize) -> anyhow::Result<()> {
    // The handle belongs to the live test window; these messages contain no pointers.
    if unsafe { PostMessageW(handle, message, wparam, lparam) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn position(point: POINT) -> isize {
    ((point.x as u16 as u32) | ((point.y as u16 as u32) << 16)) as isize
}
pub(super) async fn click_client(
    point: gpui::Point<Pixels>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    click_client_button(point, MouseButton::Left, cx).await
}
pub(super) async fn click_client_button(
    point: gpui::Point<Pixels>,
    button: MouseButton,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let (down, up, pressed) = match button {
        MouseButton::Left => (WM_LBUTTONDOWN, WM_LBUTTONUP, 1),
        MouseButton::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP, 2),
        _ => anyhow::bail!("unsupported test mouse button"),
    };
    let (handle, position) = cx.update(|window, _| -> anyhow::Result<_> {
        let scale = window.scale_factor();
        Ok((
            hwnd(window)? as usize,
            position(POINT {
                x: (f32::from(point.x) * scale).round() as i32,
                y: (f32::from(point.y) * scale).round() as i32,
            }),
        ))
    })??;
    for (message, buttons) in [(WM_MOUSEMOVE, 0), (down, pressed), (up, 0)] {
        post(handle as HWND, message, buttons, position)?;
        cx.background_executor()
            .timer(Duration::from_millis(80))
            .await;
    }
    Ok(())
}
async fn click_caption(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    id: &str,
    hit: u32,
) -> anyhow::Result<()> {
    // Native minimize/restore can finish before GPUI paints its new hit-test tree.
    // Measure the caption controls in a fresh frame before reading their coordinates.
    this.update_in(cx, |view, _, cx| {
        view.caption_bounds.clear();
        cx.notify();
    })?;
    wait(this, cx, "caption controls repainted", |view, _| {
        view.caption_bounds.len() == 3
    })
    .await?;
    let (handle, client, screen) =
        this.update_in(cx, |view, window, _| -> anyhow::Result<_> {
            let center = view
                .caption_bounds
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("caption not painted: {id}"))?
                .center();
            let handle = hwnd(window)?;
            let scale = window.scale_factor();
            let client = POINT {
                x: (f32::from(center.x) * scale).round() as i32,
                y: (f32::from(center.y) * scale).round() as i32,
            };
            let mut screen = client;
            // Convert the rendered client hitbox into the screen coordinates used by NC input.
            anyhow::ensure!(
                unsafe { ClientToScreen(handle, &mut screen) } != 0,
                "ClientToScreen failed"
            );
            Ok((handle as usize, client, screen))
        })??;
    // GPUI's caption hit test uses its latest pointer hitboxes. Do not yield between
    // the synthetic move and hit test: real cursor/leave events can replace them.
    // Both calls run outside an App update so GPUI can re-enter its callbacks.
    let actual = unsafe {
        SendMessageW(handle as HWND, WM_MOUSEMOVE, 0, position(client));
        SendMessageW(handle as HWND, WM_NCHITTEST, 0, position(screen))
    };
    anyhow::ensure!(
        actual == hit as isize,
        "caption hit-test mismatch for {id}: {actual} != {hit}"
    );
    post(
        handle as HWND,
        WM_NCLBUTTONDOWN,
        hit as usize,
        position(screen),
    )?;
    cx.background_executor()
        .timer(Duration::from_millis(80))
        .await;
    post(
        handle as HWND,
        WM_NCLBUTTONUP,
        hit as usize,
        position(screen),
    )?;
    cx.background_executor()
        .timer(Duration::from_millis(120))
        .await;
    Ok(())
}
async fn wait_window(
    cx: &mut AsyncWindowContext,
    label: &str,
    expected: impl Fn(&Window) -> bool,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if cx.update(|window, _| expected(window))? {
            return Ok(());
        }
        anyhow::ensure!(Instant::now() < deadline, "caption click failed: {label}");
        cx.background_executor()
            .timer(Duration::from_millis(30))
            .await;
    }
}
fn running_sessions(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    stage: &str,
) -> anyhow::Result<()> {
    let (running, snapshots, panes) = this.update_in(cx, |view, _, cx| {
        (
            view.panes
                .values()
                .all(|entry| entry.view.read(cx).phase == Phase::Running),
            view.runtime.supervisor().list(crate::bridge::OWNER),
            view.panes
                .values()
                .map(|entry| {
                    let pane = entry.view.read(cx);
                    (
                        pane.phase.clone(),
                        pane.error.clone(),
                        pane.test_text().chars().take(500).collect::<String>(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    })?;
    tracing::info!(stage, ?snapshots, "Native caption session state");
    anyhow::ensure!(
        running,
        "terminal stopped during {stage}: {snapshots:?}, {panes:?}"
    );
    Ok(())
}
pub(super) async fn check(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    running_sessions(this, cx, "before caption checks")?;
    click_caption(this, cx, "window-maximize", HTMAXBUTTON).await?;
    wait_window(cx, "maximize", Window::is_maximized).await?;
    running_sessions(this, cx, "maximize")?;
    click_caption(this, cx, "window-maximize", HTMAXBUTTON).await?;
    wait_window(cx, "restore", |window| !window.is_maximized()).await?;
    running_sessions(this, cx, "restore")?;
    click_caption(this, cx, "window-minimize", HTMINBUTTON).await?;
    wait_window(cx, "minimize", |window| {
        hwnd(window).is_ok_and(|h| unsafe { IsIconic(h) } != 0)
    })
    .await?;
    running_sessions(this, cx, "minimize")?;
    cx.update(|window, _| post(hwnd(window)?, WM_SYSCOMMAND, SC_RESTORE as usize, 0))??;
    wait_window(cx, "restore after minimize", |window| {
        hwnd(window).is_ok_and(|h| unsafe { IsIconic(h) } == 0)
    })
    .await?;
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    running_sessions(this, cx, "restore after minimize")?;
    click_caption(this, cx, "window-close", HTCLOSE).await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| matches!(
            view.modal,
            Some(Modal::Close(CloseTarget::Window))
        ) && !view.quitting)?,
        "caption close bypassed or missed confirmation"
    );
    key(cx, "escape")?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.modal.is_none()
            && !view.panes.is_empty()
            && !view.quitting)?,
        "caption close cancellation lost sessions"
    );
    Ok(())
}
