fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "bootstrap",
                "prepare_session",
                "start_session",
                "input",
                "reserve_input",
                "output_consumed",
                "resize_session",
                "close_session",
                "view_failed",
                "forget_session",
                "list_sessions",
                "load_config",
                "save_config",
                "save_terminal",
                "save_workspace",
                "read_clipboard",
                "write_clipboard",
                "open_link",
                "diagnostics",
            ]),
        ))
        .expect("Tauri build failed");
    }
}
