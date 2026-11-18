//! Profile creation through the rendered + button, overlay and real ConPTY sessions.
use super::{CloseTarget, Modal, Phase, Workspace, key, wait, window_controls};
use crate::bridge::OWNER;
use gpui::*;

pub(super) async fn painted(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    this.update_in(cx, |view, _, cx| {
        view.profile_bounds.clear();
        cx.notify();
    })?;
    wait(this, cx, "profile controls painted", |view, _| {
        view.profile_bounds.contains_key("new-tab")
            && (view.profile_menu.is_none() || view.profile_bounds.contains_key("menu"))
    })
    .await
}

pub(super) async fn click(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    id: &str,
) -> anyhow::Result<()> {
    painted(this, cx).await?;
    let position = this.update_in(cx, |view, _, _| view.profile_bounds[id].center())?;
    window_controls::click_client(position, cx).await
}

fn dismissed(this: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    anyhow::ensure!(
        this.update_in(cx, |view, window, cx| view.profile_menu.is_none()
            && view
                .active_view()
                .unwrap()
                .read(cx)
                .focus
                .is_focused(window))?,
        "dismissing the + menu did not return terminal focus"
    );
    Ok(())
}

async fn layout(this: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    // Fill the strip beyond 600 px so the trigger and menu must fit beside scrolling tabs.
    painted(this, cx).await?;
    for expected in 2..=4 {
        key(cx, "ctrl-shift-t")?;
        wait(this, cx, "shortcut-created tab rendered", |view, cx| {
            view.tabs.len() == expected
                && view.active_view().is_some_and(|pane| {
                    let pane = pane.read(cx);
                    pane.frames > 0
                        && pane.phase == Phase::Running
                        && pane.test_text().contains("READY")
                })
        })
        .await?;
    }
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
        let before = this.update_in(cx, |view, _, cx| {
            view.active_view().unwrap().read(cx).bounds
        })?;
        click(this, cx, "new-tab").await?;
        painted(this, cx).await?;
        let geometry = this.update_in(cx, |view, window, cx| {
            (
                view.profile_bounds["new-tab"],
                view.profile_bounds["menu"],
                before,
                view.active_view().unwrap().read(cx).bounds,
                window.viewport_size(),
                view.profile_menu.is_some(),
                view.tabs.len(),
                view.panes.len(),
            )
        })?;
        anyhow::ensure!(
            this.update_in(cx, |view, window, cx| {
                let button = view.profile_bounds["new-tab"];
                let menu = view.profile_bounds["menu"];
                let viewport = window.viewport_size();
                view.profile_menu.is_some()
                    && view.tabs.len() == 4
                    && view.panes.len() == 4
                    && view.active_view().unwrap().read(cx).bounds == before
                    && button.left() >= px(0.0)
                    && button.right() <= viewport.width
                    && menu.left() >= px(0.0)
                    && menu.right() <= viewport.width
                    && menu.top() >= button.bottom()
                    && menu.top() - button.bottom() <= px(12.0)
                    && menu.bottom() <= viewport.height
                    && menu.left() < button.right()
                    && menu.right() > button.left()
            })?,
            "+ menu clipped, moved the terminal, or lost its anchor at {viewport:?}, light={light}: {geometry:?}"
        );
        let outside =
            cx.update(|window, _| point(px(16.0), window.viewport_size().height - px(32.0)))?;
        window_controls::click_client(outside, cx).await?;
        dismissed(this, cx)?;
    }
    this.update_in(cx, |view, window, cx| {
        view.light = false;
        window.resize(original);
        while view.tabs.len() > 1 {
            view.close_confirmed(CloseTarget::Tab(view.tabs.len() - 1), window, cx);
        }
    })?;
    wait(this, cx, "temporary tabs reclaimed", |view, _| {
        view.tabs.len() == 1
            && view.panes.len() == 1
            && view
                .runtime
                .supervisor()
                .list(OWNER)
                .is_ok_and(|s| s.len() == 1)
    })
    .await
}

pub(super) async fn check(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let bounds = this.update_in(cx, |view, _, cx| {
        view.active_view().unwrap().read(cx).bounds
    })?;
    click(this, cx, "new-tab").await?;
    painted(this, cx).await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, cx| view.profile_menu.is_some()
            && view.tabs.len() == 1
            && view.panes.len() == 1
            && view.active_view().unwrap().read(cx).bounds == bounds)?,
        "clicking + created a terminal or resized the grid"
    );
    key(cx, "escape")?;
    dismissed(this, cx)?;

    click(this, cx, "new-tab").await?;
    click(this, cx, "new-tab").await?;
    dismissed(this, cx)?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.tabs.len() == 1)?,
        "clicking + again created an unwanted tab"
    );

    click(this, cx, "new-tab").await?;
    this.update_in(cx, |view, window, cx| {
        view.request_close(CloseTarget::Window, window, cx)
    })?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.profile_menu.is_none()
            && matches!(view.modal, Some(Modal::Close(CloseTarget::Window))))?,
        "window close did not supersede the + menu"
    );
    key(cx, "escape")?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.modal.is_none() && !view.quitting)?,
        "Escape did not cancel window close"
    );

    click(this, cx, "new-tab").await?;
    click(this, cx, "profile-fixture-alternate").await?;
    wait(this, cx, "mouse-selected profile ready", |view, cx| {
        view.tabs.len() == 2
            && view.panes.len() == 2
            && view.profile_menu.is_none()
            && view
                .active_id()
                .and_then(|id| view.panes.get(&id))
                .is_some_and(|entry| {
                    let pane = entry.view.read(cx);
                    entry.spec.profile_id == "fixture-alternate"
                        && pane.phase == Phase::Running
                        && pane.test_text().contains("READY")
                })
    })
    .await?;
    dismissed(this, cx)?;
    this.update_in(cx, |view, window, cx| {
        view.close_confirmed(CloseTarget::Tab(1), window, cx)
    })?;
    layout(this, cx).await?;

    click(this, cx, "new-tab").await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, _| view.profile_selection == 0)?,
        "menu did not follow the selected tab's profile"
    );
    key(cx, "down")?;
    key(cx, "enter")?;
    wait(this, cx, "keyboard-selected profile ready", |view, cx| {
        view.tabs.len() == 2
            && view.panes.len() == 2
            && view.profile_menu.is_none()
            && view
                .active_id()
                .and_then(|id| view.panes.get(&id))
                .is_some_and(|entry| {
                    let pane = entry.view.read(cx);
                    entry.spec.profile_id == "fixture-alternate"
                        && pane.phase == Phase::Running
                        && pane.test_text().contains("READY")
                        && pane.bounds.size.width > px(100.0)
                })
    })
    .await?;
    dismissed(this, cx)
}
