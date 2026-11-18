#![cfg_attr(
    all(windows, not(debug_assertions), not(feature = "test-support")),
    windows_subsystem = "windows"
)]
fn main() {
    if let Err(error) = start() {
        eprintln!("Terminder: {error:#}");
        #[cfg(all(windows, not(feature = "test-support")))]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
            let message: Vec<u16> = format!("Terminder 启动失败\n{error:#}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let title: Vec<u16> = "Terminder".encode_utf16().chain(Some(0)).collect();
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
        std::process::exit(1);
    }
}
fn start() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--help" || s == "-h") {
        println!(
            "Terminder\n  --profile <id>  Select a configured shell\n  --cwd <path>    Start in an existing absolute directory\n  --help          Print this help"
        );
        return Ok(());
    }
    #[cfg(all(windows, feature = "test-support"))]
    if args.first().is_some_and(|s| s == "--smoke") {
        anyhow::ensure!(args.len() == 3, "--smoke <pty-fixture.exe> <report.json>");
        return terminder_desktop::smoke::run(args[1].clone().into(), args[2].clone().into());
    }
    #[cfg(all(windows, feature = "test-support"))]
    if args.first().is_some_and(|s| s == "--ui-smoke") {
        anyhow::ensure!(
            args.len() == 3,
            "--ui-smoke <pty-fixture.exe> <report.json>"
        );
        return terminder_desktop::native::run_smoke(
            args[1].clone().into(),
            args[2].clone().into(),
        );
    }
    #[cfg(all(windows, feature = "test-support"))]
    if args.first().is_some_and(|s| s == "--restore-smoke") {
        anyhow::ensure!(
            args.len() == 3,
            "--restore-smoke <previous-report.json> <report.json>"
        );
        return terminder_desktop::native::run_restore_smoke(
            args[1].clone().into(),
            args[2].clone().into(),
        );
    }
    let (mut profile, mut cwd) = (None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--profile" => {
                profile = Some(
                    iter.next()
                        .ok_or_else(|| anyhow::anyhow!("--profile requires a value"))?
                        .clone(),
                )
            }
            "--cwd" => {
                cwd = Some(std::path::PathBuf::from(
                    iter.next()
                        .ok_or_else(|| anyhow::anyhow!("--cwd requires a value"))?,
                ))
            }
            _ => anyhow::bail!("unknown argument: {arg}"),
        }
    }
    #[cfg(windows)]
    {
        terminder_session_core::backend::ensure_supported_platform()?;
        let paths = terminder_desktop::config::store::Paths::discover()?;
        terminder_desktop::logging::initialize(&paths.logs)?;
        terminder_desktop::native::run(paths, profile, cwd)
    }
    #[cfg(not(windows))]
    {
        let _ = (profile, cwd);
        anyhow::bail!("Terminder requires Windows")
    }
}
