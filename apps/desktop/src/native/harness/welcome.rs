//! Welcome and last-tab behavior against the real GPUI window and ConPTY runtime.
use super::{CloseTarget, Modal, Phase, Workspace, command, key, wait};
use crate::{bridge::OWNER, native::workspace::ProfileMenu};
use gpui::*;
use std::time::Duration;

pub(super) async fn idle(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    wait(this, cx, "welcome has no remaining sessions", |view, _| {
        view.tabs.is_empty()
            && view.panes.is_empty()
            && view.active_id().is_none()
            && !view.quitting
            && view.modal.is_none()
            && view.profile_menu.is_none()
            && view
                .runtime
                .supervisor()
                .list(OWNER)
                .is_ok_and(|s| s.is_empty())
    })
    .await?;
    anyhow::ensure!(
        this.update_in(cx, |view, window, _| view.focus.is_focused(window))?,
        "welcome did not receive keyboard focus"
    );
    Ok(())
}

async fn ready(this: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    wait(this, cx, "chosen terminal is ready", |view, cx| {
        view.tabs.len() == 1
            && view.active_view().is_some_and(|p| {
                let pane = p.read(cx);
                pane.phase == Phase::Running
                    && pane.frames > 0
                    && pane.test_text().contains("READY")
            })
    })
    .await
}

async fn painted(this: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    this.update_in(cx, |view, _, cx| {
        view.welcome_bounds.clear();
        cx.notify();
    })?;
    wait(this, cx, "welcome controls painted", |view, _| {
        view.welcome_bounds.len() == 3
    })
    .await
}

async fn click(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    id: &str,
) -> anyhow::Result<()> {
    painted(this, cx).await?;
    let position = this.update_in(cx, |view, _, _| view.welcome_bounds[id].center())?;
    super::window_controls::click_client(position, cx).await
}

async fn layout(this: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let original = cx.update(|window, _| window.viewport_size())?;
    for (viewport, light) in [
        (original, false),
        (size(px(600.0), px(400.0)), false),
        (size(px(600.0), px(400.0)), true),
    ] {
        this.update_in(cx, |view, window, cx| {
            view.light = light;
            window.resize(viewport);
            cx.notify();
        })?;
        painted(this, cx).await?;
        anyhow::ensure!(
            this.update_in(cx, |view, _, _| {
                let surface = view.welcome_bounds["surface"];
                ["body", "new"].iter().all(|id| {
                    let bounds = view.welcome_bounds[id];
                    bounds.size.width > px(40.0)
                        && bounds.size.height > px(20.0)
                        && bounds.left() >= surface.left()
                        && bounds.right() <= surface.right()
                        && bounds.top() >= surface.top()
                        && bounds.bottom() <= surface.bottom()
                })
            })?,
            "welcome content was clipped at {viewport:?}, light={light}"
        );
        click(this, cx, "new").await?;
        super::profiles::painted(this, cx).await?;
        let geometry = this.update_in(cx, |view, window, _| {
            (
                view.welcome_bounds["new"],
                view.profile_bounds["menu"],
                window.viewport_size(),
            )
        })?;
        anyhow::ensure!(
            this.update_in(cx, |view, window, _| {
                let button = view.welcome_bounds["new"];
                let menu = view.profile_bounds["menu"];
                let viewport = window.viewport_size();
                let below =
                    menu.top() >= button.bottom() && menu.top() - button.bottom() <= px(12.0);
                let above =
                    menu.bottom() <= button.top() && button.top() - menu.bottom() <= px(12.0);
                view.profile_menu == Some(ProfileMenu::Welcome)
                    && view.tabs.is_empty()
                    && view.panes.is_empty()
                    && view
                        .runtime
                        .supervisor()
                        .list(OWNER)
                        .is_ok_and(|s| s.is_empty())
                    && menu.left() >= px(0.0)
                    && menu.right() <= viewport.width
                    && menu.top() >= px(0.0)
                    && menu.bottom() <= viewport.height
                    && menu.left() < button.right()
                    && menu.right() > button.left()
                    && (above || below)
            })?,
            "welcome menu started a session or lost its button anchor: {geometry:?}"
        );
        if viewport == original {
            click(this, cx, "new").await?;
        } else if light {
            super::window_controls::click_client(point(px(16.0), viewport.height - px(32.0)), cx)
                .await?;
        } else {
            key(cx, "escape")?;
        }
        idle(this, cx).await?;
    }
    this.update_in(cx, |view, window, cx| {
        view.light = false;
        window.resize(original);
        cx.notify();
    })?;
    painted(this, cx).await
}

pub(super) async fn check(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    cases: &mut Vec<String>,
) -> anyhow::Result<()> {
    // Wait for the first rendered event tree before dispatching workspace shortcuts.
    wait(this, cx, "initial window render", |view, _| {
        view.caption_bounds.len() == 3
    })
    .await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.tabs.is_empty()
            && view.panes.is_empty())?,
        "ordinary startup unexpectedly opened a terminal"
    );
    idle(this, cx).await?;
    layout(this, cx).await?;
    cases.push("One welcome New terminal button opened a nearby menu without sessions; normal/600x400 dark/light layouts, repeat-click, Escape and outside-click dismissal passed".into());
    super::profiles::click(this, cx, "new-tab").await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.profile_menu.is_some()
            && view.tabs.is_empty()
            && view.panes.is_empty()
            && view
                .runtime
                .supervisor()
                .list(OWNER)
                .is_ok_and(|s| s.is_empty()))?,
        "+ on welcome started a terminal before a profile was chosen"
    );
    key(cx, "escape")?;
    idle(this, cx).await?;
    cases.push("Clicking + on welcome opened the profile menu with zero sessions; Escape returned to welcome".into());
    key(cx, "ctrl-,")?;
    wait(this, cx, "settings on welcome", |view, _| {
        matches!(view.modal, Some(Modal::Settings))
    })
    .await?;
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    key(cx, "escape")?;
    idle(this, cx).await?;
    key(cx, "ctrl-shift-w")?;
    key(cx, "ctrl-alt-w")?;
    idle(this, cx).await?;
    cases.push("Startup displayed welcome with zero ConPTY sessions; settings and empty-close shortcuts kept the window usable".into());

    click(this, cx, "new").await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.profile_menu
            == Some(ProfileMenu::Welcome)
            && view.tabs.is_empty()
            && view.panes.is_empty())?,
        "welcome New terminal bypassed profile selection"
    );
    super::profiles::click(this, cx, "profile-fixture").await?;
    ready(this, cx).await?;
    key(cx, "ctrl-shift-w")?;
    wait(this, cx, "last tab close confirmation", |view, _| {
        matches!(view.modal, Some(Modal::Close(CloseTarget::Tab(0))))
    })
    .await?;
    key(cx, "escape")?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.tabs.len() == 1
            && view.modal.is_none()
            && !view.quitting)?,
        "cancelling last tab close removed the session"
    );
    key(cx, "ctrl-shift-w")?;
    key(cx, "enter")?;
    idle(this, cx).await?;
    cases.push("Clicked welcome New terminal then a profile row; exactly one terminal started; last running tab cancel retained the session; confirm returned to welcome and reclaimed the real ConPTY without exiting".into());

    click(this, cx, "new").await?;
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    key(cx, "down")?;
    key(cx, "enter")?;
    ready(this, cx).await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view
            .active_id()
            .and_then(|id| view.panes.get(&id))
            .is_some_and(|entry| entry.spec.profile_id == "fixture-alternate"))?,
        "welcome did not open the selected profile"
    );
    key(cx, "ctrl-alt-w")?;
    wait(this, cx, "last pane close confirmation", |view, _| {
        matches!(view.modal, Some(Modal::Close(CloseTarget::Pane(_))))
    })
    .await?;
    key(cx, "enter")?;
    idle(this, cx).await?;
    cases.push("Clicked the same welcome New terminal button; keyboard selected the alternative Shell; closing the final pane returned to welcome and reclaimed its session".into());

    this.update_in(cx, |view, _, _| {
        view.document.config.session.confirm_close_running = false
    })?;
    key(cx, "ctrl-shift-t")?;
    ready(this, cx).await?;
    key(cx, "ctrl-shift-w")?;
    idle(this, cx).await?;
    this.update_in(cx, |view, _, _| {
        view.document.config.session.confirm_close_running = true
    })?;
    cases.push("With close confirmation disabled, closing the last tab still kept the application and allowed another new terminal".into());
    command(this, cx, "new_tab")?;
    ready(this, cx).await?;
    this.update_in(cx, |view, _, cx| {
        view.active_view()
            .unwrap()
            .update(cx, |pane, _| pane.test_input(b"quit\r".to_vec()));
    })?;
    wait(this, cx, "final terminal exited naturally", |view, cx| {
        view.active_view().unwrap().read(cx).phase == Phase::Finished
    })
    .await?;
    key(cx, "ctrl-shift-w")?;
    idle(this, cx).await?;
    cases.push("Closing the final already-exited terminal returned to welcome without a running-session confirmation".into());
    command(this, cx, "new_tab")?;
    ready(this, cx).await
}

pub(super) async fn cancel_restore(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    cases: &mut Vec<String>,
) -> anyhow::Result<()> {
    wait(this, cx, "restore confirmation rendered", |view, _| {
        view.caption_bounds.len() == 3
    })
    .await?;
    key(cx, "escape")?;
    idle(this, cx).await?;
    cases.push("Declining startup restore left welcome with zero sessions instead of opening a fallback Shell".into());
    let config = this.update_in(cx, |view, window, cx| {
        let config = view.document.config.clone();
        for profile in &mut view.document.config.profiles {
            profile.id = format!("renamed-{}", profile.id);
        }
        view.document.config.default_profile = view.document.config.profiles[0].id.clone();
        view.restore(window, cx);
        config
    })?;
    idle(this, cx).await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.status.contains("无法恢复"))?,
        "unavailable saved profiles did not report a restore error"
    );
    cases.push("Unavailable saved profiles reported errors and kept welcome without starting an unrelated default Shell".into());
    this.update_in(cx, |view, _, _| {
        view.document.config = config;
        view.status.clear();
    })?;
    Ok(())
}
