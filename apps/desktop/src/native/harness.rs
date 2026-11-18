//! Executable GUI checks; this module is absent from ordinary builds.
mod clipboard;
mod profiles;
mod welcome;
mod window_controls;

use super::{
    input::InputEvent,
    workspace::{CloseTarget, Command, Modal, Workspace},
};
use crate::config::{model::Config, store::Paths};
use gpui::*;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use terminder_session_core::{model::Phase, profile::Profile};

pub fn paths(fixture: &Path, report: &Path) -> anyhow::Result<Paths> {
    let base = report
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report requires a parent directory"))?
        .join(format!("ui-data-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base)?;
    let paths = Paths {
        config: base.join("native.toml"),
        state: base.join("native-workspace.json"),
        logs: base.join("logs"),
    };
    let mut config = Config {
        default_profile: "fixture".into(),
        ..Config::default()
    };
    config.window.restore_tabs = true;
    config.terminal.theme = "dark".into();
    config.profiles = vec![Profile {
        id: "fixture".into(),
        name: "Isolated native UI".into(),
        platforms: vec!["windows".into()],
        resolver: None,
        program: Some(fixture.canonicalize()?),
        args: vec!["echo".into()],
        cwd_policy: "fixed".into(),
        cwd: Some(std::env::current_dir()?),
        env: Default::default(),
    }];
    let mut alternate = config.profiles[0].clone();
    alternate.id = "fixture-alternate".into();
    alternate.name = "Alternative native UI".into();
    config.profiles.push(alternate);
    std::fs::write(&paths.config, toml::to_string_pretty(&config)?)?;
    Ok(paths)
}
async fn wait(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    description: &str,
    condition: impl Fn(&Workspace, &App) -> bool,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if this.update_in(cx, |view, _, cx| condition(view, cx))? {
            return Ok(());
        }
        anyhow::ensure!(Instant::now() < deadline, "UI timeout: {description}");
        cx.background_executor()
            .timer(Duration::from_millis(30))
            .await;
    }
}
fn command(
    this: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
    name: &str,
) -> anyhow::Result<()> {
    this.update_in(cx, |view, window, cx| {
        view.command(&Command { name: name.into() }, window, cx)
    })?;
    Ok(())
}
fn key(cx: &mut AsyncWindowContext, key: &str) -> anyhow::Result<()> {
    let keystroke = Keystroke::parse(key)?;
    cx.update(|window, cx| {
        window.dispatch_keystroke(keystroke, cx);
    })?;
    Ok(())
}
pub fn start(window: &Window, report: PathBuf, cx: &mut Context<Workspace>) {
    cx.spawn_in(window,async move|this,cx|{
        let mut cases=Vec::<String>::new();let started=Instant::now();
        let result=async {
            if this.update_in(cx,|view,_,_|matches!(view.modal,Some(Modal::Restore)))? {
                welcome::cancel_restore(&this, cx, &mut cases).await?;
                this.update_in(cx,|view,window,cx|view.restore(window,cx))?;
                wait(&this,cx,"restored native workspace",|view,cx|view.tabs.len()==2&&view.panes.len()==3&&view.panes.values().all(|entry|{let pane=entry.view.read(cx);pane.phase==Phase::Running&&pane.error.is_none()&&pane.test_text().contains("READY")})&&view.active_view().is_some_and(|pane|pane.read(cx).frames>0)).await?;
                anyhow::ensure!(this.update_in(cx,|view,_,_|view.tabs[view.active].title.as_deref()==Some("GPUI native")&&view.tabs[view.active].layout.ids().len()==2)?,"restored title or nested layout mismatch");
                cases.push("New application process confirmed restore; rebuilt two tabs, three ConPTYs, saved layout, active pane and title".into());
                this.update_in(cx, |view, window, cx| {
                    let tab_count = view.tabs.len();
                    for _ in 0..tab_count {
                        view.close_confirmed(CloseTarget::Tab(view.active), window, cx);
                    }
                })?;
                welcome::idle(&this, cx).await?;
                cases.push("Closing all restored tabs returned to welcome; closing this empty window then saved an empty workspace".into());
                return Ok(());
            }
            tracing::info!("Native UI check starting: welcome");
            welcome::check(&this, cx, &mut cases).await?;
            tracing::info!("Native UI check finished: welcome");
            wait(&this,cx,"initial terminal frame",|view,cx|view.panes.len()==1&&view.panes.values().all(|e|{let p=e.view.read(cx);p.phase==Phase::Running&&p.frames>0&&p.bounds.size.width>px(100.0)&&p.test_text().contains("READY")})).await?;
            cases.push("Created real GPUI window; terminal canvas prepainted; ConPTY ready".into());
            clipboard::check(&this, cx).await?;
            cases.push("Native selection, Ctrl+C/right-click Unicode copy, explicit/custom copy, empty-selection interrupt, right-click paste and Windows clipboard readback".into());
            tracing::info!("Native UI check starting: window controls");
            window_controls::check(&this, cx).await?;
            tracing::info!("Native UI check finished: window controls");
            cases.push("Native caption mouse messages minimized, maximized, restored and opened/cancelled close confirmation".into());
            this.update_in(cx,|view,window,cx|{let pane=view.active_view().unwrap();pane.update(cx,|pane,cx|{pane.replace_and_mark_text_in_range(None,"UI_中文_🌱",None,window,cx);pane.replace_text_in_range(None,"UI_中文_🌱",window,cx);pane.test_input(vec![13]);});})?;
            wait(&this,cx,"native UTF-16 commit",|view,cx|view.active_view().unwrap().read(cx).test_text().contains("ECHO:UI_中文_🌱")).await?;
            cases.push("Native EntityInputHandler composition and commit produced Unicode echo".into());
            let frames=this.update_in(cx,|view,_,cx| { let pane=view.active_view().unwrap();let frames=pane.read(cx).frames;pane.update(cx,|pane,cx|pane.test_title(&"中文路径🌱".repeat(20),cx)).unwrap();cx.notify();frames })?;
            wait(&this,cx,"Unicode title redraw",|view,cx|view.active_view().unwrap().read(cx).frames>frames).await?;
            cases.push("Repainted a long mixed Unicode/emoji title without UTF-8 truncation panic".into());
            this.update_in(cx,|view,_,cx|{view.active_view().unwrap().update(cx,|pane,cx|pane.test_stage_paste("PASTE_SAFE_A\nPASTE_SAFE_B\n".into(),cx));})?;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            anyhow::ensure!(this.update_in(cx,|view,_,cx|{let pane=view.active_view().unwrap();let pane=pane.read(cx);pane.test_paste_pending()&&!pane.test_text().contains("PASTE_SAFE_A")})?,"multiline paste reached PTY before confirmation");
            command(&this,cx,"paste")?;
            wait(&this,cx,"confirmed paste",|view,cx|view.active_view().unwrap().read(cx).test_text().contains("ECHO:PASTE_SAFE_B")).await?;
            cases.push("Multiline paste withheld bytes until confirmation, then reached real ConPTY without touching system clipboard".into());
            tracing::info!("Native UI check starting: profile menu");
            profiles::check(&this,cx).await?;
            tracing::info!("Native UI check finished: profile menu");
            cases.push("+ opened only the menu; mouse and keyboard created the selected profile; grid, focus, window-close confirmation, dark/light layout and narrow tab overflow remained correct".into());
            wait(&this,cx,"second tab",|view,cx|view.tabs.len()==2&&view.active_view().unwrap().read(cx).bounds.size.width>px(100.0)).await?;
            command(&this,cx,"split_right")?;
            wait(&this,cx,"horizontal split",|view,cx|view.panes.len()==3&&view.active_view().unwrap().read(cx).frames>0).await?;
            command(&this,cx,"split_down")?;
            wait(&this,cx,"nested split",|view,cx|view.panes.len()==4&&view.panes.values().all(|e|{let pane=e.view.read(cx);pane.phase==Phase::Running&&pane.test_text().contains("READY")})).await?;
            this.update_in(cx,|view,_,cx|{view.tabs[view.active].layout.set_ratio(&[],0.55);cx.notify();})?;
            command(&this,cx,"focus_left")?;
            let original=this.update_in(cx,|view,_,_|view.active_id().unwrap())?;
            command(&this,cx,"focus_right")?;
            let changed=this.update_in(cx,|view,_,_|view.active_id().unwrap())?;
            anyhow::ensure!(original!=changed,"directional focus did not move");
            cases.push("Created two tabs and nested horizontal/vertical splits; changed ratio and directional focus".into());
            this.update_in(cx,|view,window,cx|{view.active_view().unwrap().update(cx,|pane,cx|{pane.replace_text_in_range(None,"SEARCH_UNIQUE_中文",window,cx);pane.test_input(vec![13]);});})?;
            wait(&this,cx,"search text",|view,cx|view.active_view().unwrap().read(cx).test_text().contains("ECHO:SEARCH_UNIQUE_中文")).await?;
            this.update_in(cx,|view,window,cx|{view.open_search(window,cx);view.search.as_ref().unwrap().update(cx,|input,_|{input.buffer=crate::text::TextBuffer::new("SEARCH_UNIQUE_中文".into());});view.find(false,cx);})?;
            let selection=this.update_in(cx,|view,_,cx|view.active_view().unwrap().read(cx).test_selection())?;
            anyhow::ensure!(selection.as_deref()==Some("SEARCH_UNIQUE_中文"),"search selection mismatch");
            this.update_in(cx,|view,window,cx|{view.close_search(window,cx);view.rename(window,cx);if let Some(Modal::Rename(input))=&view.modal{input.update(cx,|input,cx|{input.buffer=crate::text::TextBuffer::new("GPUI native".into());cx.emit(InputEvent::Submit);});}})?;
            wait(&this,cx,"rename event",|view,_|view.tabs[view.active].title.as_deref()==Some("GPUI native")).await?;
            cases.push("Native search selected Unicode match; rename field committed through subscription".into());
            this.update_in(cx,|view,window,cx|{view.open_settings(window,cx);let settings=view.settings.as_mut().unwrap();settings.terminal.theme="light".into();settings.terminal.font_size_px=16.0;view.save_settings(window,cx);})?;
            wait(&this,cx,"save settings",|view,cx|!view.config_busy&&view.document.config.terminal.font_size_px==16.0&&view.panes.values().all(|e|e.view.read(cx).light)).await?;
            cases.push("Saved native appearance atomically and applied light theme/font to all panes".into());
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let (id, active_before)=this.update_in(cx,|view,window,cx|{
                let active=view.active_id().unwrap();
                let id=*view.tabs[view.active].layout.ids().iter().find(|id|**id!=active).unwrap();
                view.request_close(CloseTarget::Pane(id),window,cx);
                (id,active)
            })?;
            anyhow::ensure!(this.update_in(cx,|view,_,_|matches!(view.modal,Some(Modal::Close(CloseTarget::Pane(target))) if target==id)&&view.active_id()==Some(active_before))?,"inactive close changed selection or target");
            anyhow::ensure!(this.update_in(cx,|view,_,_|matches!(view.modal,Some(Modal::Close(_)))&&view.panes.len()==4)?,"running close did not request confirmation");
            this.update_in(cx,|view,window,cx|view.cancel_modal(window,cx))?;
            let before=Instant::now();this.update_in(cx,|view,window,cx|view.close_confirmed(CloseTarget::Pane(id),window,cx))?;
            anyhow::ensure!(before.elapsed()<Duration::from_secs(1),"pane close blocked UI thread");
            anyhow::ensure!(this.update_in(cx,|view,_,_|view.panes.len()==3&&!view.panes.contains_key(&id))?,"closed pane retained");
            cases.push("Inactive pane close targets its own session without changing selection; confirmation/cancel; asynchronous close".into());
            this.update_in(cx,|view,_,cx|{view.active_view().unwrap().update(cx,|pane,_|pane.test_input(b"quit\r".to_vec()));})?;
            wait(&this,cx,"natural session exit",|view,cx|view.active_view().unwrap().read(cx).phase==Phase::Finished).await?;
            command(&this,cx,"restart")?;
            wait(&this,cx,"restart finished pane",|view,cx|{let pane=view.active_view().unwrap().read(cx);pane.phase==Phase::Running&&pane.test_text().contains("READY")}).await?;
            cases.push("Retained exited pane and restarted it in place".into());
            let original_size=cx.update(|window,_| {let original=window.viewport_size();window.resize(size(px(600.0),px(400.0)));original})?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            anyhow::ensure!(this.update_in(cx,|view,_,cx|view.tabs[view.active].layout.ids().iter().all(|id|{let bounds=view.panes[id].view.read(cx).bounds;bounds.size.width>px(100.0)&&bounds.size.height>px(50.0)}))?,"minimum viewport lost a visible pane");
            cx.update(|window,_|window.resize(original_size))?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            anyhow::ensure!(cx.update(|window,_|window.viewport_size()==original_size)?,"window client size did not return to original");
            // Caption maximize/restore is exercised by native mouse messages above.
            cx.update(|window,_|window.zoom_window())?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            anyhow::ensure!(cx.update(|window,_|window.is_maximized())?,"custom window failed to maximize");
            cases.push("Minimum 600x400 content retained panes; original client size restored; native maximize with custom titlebar".into());
            Ok::<(),anyhow::Error>(())
        }.await;
        let state_path=this.update_in(cx,|view,_,_|view.paths.state.clone()).ok();
        let panes=this.update_in(cx,|view,_,cx|view.panes.iter().map(|(id,entry)|{let pane=entry.view.read(cx);serde_json::json!({"id":id,"phase":format!("{:?}",pane.phase),"error":pane.error,"text":pane.test_text().chars().take(10000).collect::<String>(),"frames":pane.frames})}).collect::<Vec<_>>()).unwrap_or_default();
        let data=serde_json::json!({"panes":panes,"passed":result.is_ok(),"cases":cases,"error":result.err().map(|e|format!("{e:#}")),"elapsed_ms":started.elapsed().as_millis(),"state_path":state_path,"physical_input_tested":false,"screenshot_tested":false});
        if let Some(parent)=report.parent(){let _=std::fs::create_dir_all(parent);}
        if let Err(error)=std::fs::write(&report,serde_json::to_vec_pretty(&data).unwrap()){tracing::error!(%error,"UI report write failed");}
        let _=this.update_in(cx,|view,window,cx|view.close_confirmed(CloseTarget::Window,window,cx));
    }).detach();
}
