//! The OS reports renderer death. A delayed ACK or an unresponsive renderer is not proof of death.
use crate::{app::AppState, platform::windows};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, ProcessFailedEventHandler};

pub fn monitor(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let supervisor = window.state::<AppState>().supervisor.clone();
    let epoch = window.state::<AppState>().window_epoch.clone();
    window.with_webview(move |platform| {
        let on_failure=supervisor.clone();
        let reported_epoch = AtomicU64::new(u64::MAX);
        let handler=ProcessFailedEventHandler::create(Box::new(move |_sender,args| {
            if let Some(args)=args {
                let mut kind=COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;
                unsafe { args.ProcessFailedKind(&mut kind)?; }
                if kind==COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED || kind==COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED || kind==COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED {
                    let owner = epoch.load(Ordering::Acquire);
                    if on_failure.is_stopped() || reported_epoch.swap(owner, Ordering::AcqRel) == owner { return Ok(()); }
                    on_failure.view_lost(&format!("main:{owner}"));
                    tauri::async_runtime::spawn_blocking(||windows::show_error("终端视图进程已退出。相关会话正在回收；请关闭窗口后重新启动 Terminder。原进程和未显示的终端内容不能恢复。"));
                }
            }
            Ok(())
        }));
        let mut token=0;
        // The WebView owns this registration until destruction. No COM pointer leaves its UI thread.
        let result=unsafe { platform.controller().CoreWebView2().and_then(|webview|webview.add_ProcessFailed(&handler,&mut token)) };
        if result.is_err() {
            supervisor.shutdown();
            tauri::async_runtime::spawn_blocking(||windows::show_error("无法注册 WebView2 进程故障监控，已停止会话服务。请修复 WebView2 Runtime 后重启应用。"));
        }
    })
}
