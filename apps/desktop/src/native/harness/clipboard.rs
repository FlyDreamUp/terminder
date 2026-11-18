//! Selection and copying through the rendered terminal and system clipboard.
use super::{Phase, Workspace, command, key, wait, window_controls};
use crate::native::workspace::bind_shortcuts;
use gpui::*;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};

const UNICODE: &str = "COPY_中文_🌱";
const SENTINEL: &str = "COPY_SENTINEL";

fn clipboard_is(cx: &mut AsyncWindowContext, expected: &str) -> anyhow::Result<bool> {
    cx.update(|_, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref()
            == Some(expected)
    })
}
async fn expect_clipboard(cx: &mut AsyncWindowContext, expected: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if clipboard_is(cx, expected)? {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "clipboard did not contain expected fixture text: {expected}"
        );
        cx.background_executor()
            .timer(Duration::from_millis(20))
            .await;
    }
}
fn seed_clipboard(cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    cx.update(|window, _| crate::native::clipboard::write_text(window, SENTINEL))?
}
fn selected(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<Option<String>> {
    this.update_in(cx, |view, _, cx| {
        view.active_view().unwrap().read(cx).test_selection()
    })
}
fn select_unicode(this: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    this.update_in(cx, |view, _, cx| {
        view.active_view()
            .unwrap()
            .update(cx, |pane, cx| pane.find(UNICODE, false, cx));
    })?;
    anyhow::ensure!(
        selected(this, cx)?.as_deref() == Some(UNICODE),
        "Unicode selection mismatch"
    );
    seed_clipboard(cx)
}

pub(super) async fn check(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let previous = cx.update(|_, cx| cx.read_from_clipboard())?;
    let result = check_copy(this, cx).await;
    // Restore the captured item even when an assertion fails; never log its contents.
    cx.update(|window, cx| -> anyhow::Result<()> {
        match previous {
            Some(item) => {
                if let Some(text) = item.text() {
                    crate::native::clipboard::write_text(window, &text)?;
                } else {
                    cx.write_to_clipboard(item);
                }
            }
            None => crate::native::clipboard::write_text(window, "")?,
        }
        Ok(())
    })??;
    result
}

async fn check_copy(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let position = this.update_in(cx, |view, _, cx| {
        view.active_view().unwrap().read(cx).bounds.origin + point(px(1.0), px(1.0))
    })?;
    window_controls::click_client(position, cx).await?;
    window_controls::click_client(position, cx).await?;
    anyhow::ensure!(
        selected(this, cx)?.as_deref() == Some("READY"),
        "terminal mouse selection did not select READY"
    );
    seed_clipboard(cx)?;
    key(cx, "ctrl-shift-c")?;
    expect_clipboard(cx, "READY").await?;
    anyhow::ensure!(
        selected(this, cx)?.as_deref() == Some("READY"),
        "explicit copy cleared the selection"
    );
    seed_clipboard(cx)?;
    key(cx, "ctrl-c")?;
    expect_clipboard(cx, "READY").await?;
    anyhow::ensure!(
        selected(this, cx)?.is_none(),
        "Ctrl+C did not clear the copied selection"
    );

    // A subsequent echo proves that copying did not send an interrupt to ConPTY.
    this.update_in(cx, |view, _, cx| {
        view.active_view().unwrap().update(cx, |pane, _| {
            pane.test_input(format!("{UNICODE}\r").into_bytes())
        });
    })?;
    wait(
        this,
        cx,
        "shell remains alive after selection copy",
        |view, cx| {
            let pane = view.active_view().unwrap().read(cx);
            pane.phase == Phase::Running && pane.test_text().contains(&format!("ECHO:{UNICODE}"))
        },
    )
    .await?;
    select_unicode(this, cx)?;
    key(cx, "ctrl-c")?;
    expect_clipboard(cx, UNICODE).await?;
    anyhow::ensure!(
        selected(this, cx)?.is_none(),
        "Ctrl+C truncated Unicode or retained the selection"
    );

    select_unicode(this, cx)?;
    {
        let _held = ClipboardLock::acquire()?;
        key(cx, "ctrl-c")?;
        anyhow::ensure!(
            selected(this, cx)?.as_deref() == Some(UNICODE),
            "failed copy discarded the selection"
        );
        anyhow::ensure!(
            this.update_in(cx, |view, _, cx| {
                view.active_view()
                    .unwrap()
                    .read(cx)
                    .test_status()
                    .starts_with("复制失败：")
            })?,
            "failed copy did not show an error"
        );
    }
    expect_clipboard(cx, SENTINEL).await?;
    key(cx, "ctrl-c")?;
    expect_clipboard(cx, UNICODE).await?;
    anyhow::ensure!(
        selected(this, cx)?.is_none(),
        "copy did not recover after clipboard contention"
    );

    select_unicode(this, cx)?;
    this.update_in(cx, |view, _, cx| {
        view.document
            .config
            .shortcuts
            .insert("copy".into(), "ctrl-alt-c".into());
        bind_shortcuts(&view.document.config, cx);
    })?;
    key(cx, "ctrl-alt-c")?;
    expect_clipboard(cx, UNICODE).await?;
    anyhow::ensure!(
        selected(this, cx)?.as_deref() == Some(UNICODE),
        "custom copy shortcut did not preserve its behavior"
    );
    this.update_in(cx, |view, _, cx| {
        view.document
            .config
            .shortcuts
            .insert("copy".into(), "ctrl-shift-c".into());
        bind_shortcuts(&view.document.config, cx);
    })?;
    seed_clipboard(cx)?;
    this.update_in(cx, |view, _, cx| {
        view.active_view().unwrap().update(cx, |pane, cx| {
            pane.test_stage_paste("STALE_UNCONFIRMED\n".into(), cx);
        });
    })?;
    window_controls::click_client_button(position, MouseButton::Right, cx).await?;
    anyhow::ensure!(
        this.update_in(cx, |view, _, cx| {
            !view.active_view().unwrap().read(cx).test_paste_pending()
        })?,
        "copy retained a stale paste confirmation"
    );
    expect_clipboard(cx, UNICODE).await?;
    anyhow::ensure!(
        selected(this, cx)?.is_none(),
        "right-click did not copy and clear the Unicode selection"
    );
    window_controls::click_client_button(position, MouseButton::Right, cx).await?;
    this.update_in(cx, |view, _, cx| {
        view.active_view()
            .unwrap()
            .update(cx, |pane, _| pane.test_input(vec![13]));
    })?;
    wait(
        this,
        cx,
        "right-click pastes after selection copy",
        |view, cx| {
            view.active_view()
                .unwrap()
                .read(cx)
                .test_text()
                .matches(&format!("ECHO:{UNICODE}"))
                .count()
                == 2
        },
    )
    .await?;

    let blank = this.update_in(cx, |view, _, cx| {
        let bounds = view.active_view().unwrap().read(cx).bounds;
        point(bounds.left() + px(1.0), bounds.bottom() - px(1.0))
    })?;
    window_controls::click_client(blank, cx).await?;
    anyhow::ensure!(
        selected(this, cx)?.is_none_or(|text| text.is_empty()),
        "single click unexpectedly selected text"
    );
    seed_clipboard(cx)?;
    key(cx, "ctrl-shift-c")?;
    expect_clipboard(cx, SENTINEL).await?;
    key(cx, "ctrl-c")?;
    wait(
        this,
        cx,
        "Ctrl+C without text still interrupts the shell",
        |view, cx| view.active_view().unwrap().read(cx).phase == Phase::Finished,
    )
    .await?;
    expect_clipboard(cx, SENTINEL).await?;
    command(this, cx, "restart")?;
    wait(
        this,
        cx,
        "restarted shell after interrupt regression",
        |view, cx| {
            let pane = view.active_view().unwrap().read(cx);
            pane.phase == Phase::Running && pane.test_text().contains("READY") && pane.frames > 0
        },
    )
    .await
}

struct ClipboardLock {
    release: mpsc::SyncSender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl ClipboardLock {
    fn acquire() -> anyhow::Result<Self> {
        let (ready, opened) = mpsc::sync_channel(1);
        let (release, waiting) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            for attempt in 0..10 {
                // Hold the clipboard on a separate thread without changing its contents.
                if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                    let _ = ready.send(Ok(()));
                    let _ = waiting.recv_timeout(Duration::from_secs(5));
                    unsafe {
                        CloseClipboard();
                    }
                    return;
                }
                if attempt == 9 {
                    let _ = ready.send(Err(std::io::Error::last_os_error()));
                    return;
                }
                thread::sleep(Duration::from_millis(5));
            }
        });
        let lock = Self {
            release,
            worker: Some(worker),
        };
        opened.recv_timeout(Duration::from_secs(1))??;
        Ok(lock)
    }
}
impl Drop for ClipboardLock {
    fn drop(&mut self) {
        let _ = self.release.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
