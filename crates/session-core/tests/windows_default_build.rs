#![cfg(all(windows, not(feature = "test-fixture")))]
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use terminder_session_core::{
    backend::{ensure_supported_platform, native_factory},
    model::*,
    supervisor::{Sink, Supervisor},
};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);
impl Sink for Capture {
    fn output(&self, bytes: Vec<u8>) -> Result<()> {
        self.0.lock().unwrap().extend(bytes);
        Ok(())
    }
    fn event(&self, _: SessionEvent) -> Result<()> {
        Ok(())
    }
}

#[test]
fn default_build_runs_a_real_windows_shell() {
    ensure_supported_platform().expect("test host must meet the ordinary build baseline");
    let directory = tempfile::tempdir().unwrap();
    let system_root = std::env::var_os("SystemRoot").expect("Windows system directory");
    let spec = SpawnSpec {
        profile_id: "default-build-test".into(),
        profile_name: "Default build test".into(),
        program: std::path::PathBuf::from(system_root)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        args: [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::WriteLine('TERMINDER_DEFAULT_BUILD_OK')",
        ]
        .map(Into::into)
        .into(),
        env: BTreeMap::new(),
        cwd: directory.path().to_path_buf(),
        cwd_source: "explicit".into(),
        size: TerminalSize::default(),
    };
    let supervisor = Supervisor::new(native_factory(), 1).unwrap();
    let handle = supervisor.prepare("main", spec).unwrap();
    let capture = Arc::new(Capture::default());
    supervisor
        .start(
            "main",
            handle.binding.clone(),
            TerminalSize::default(),
            capture.clone(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = supervisor.list("main").unwrap().remove(0);
        if snapshot.phase == Phase::Finished {
            let output = capture.0.lock().unwrap();
            let output = String::from_utf8_lossy(&output);
            assert_eq!(snapshot.exit_code, Some(0), "{snapshot:?}\n{output}");
            assert_eq!(snapshot.reason, Some(FinishReason::Exited { code: 0 }));
            assert!(snapshot.tail_complete);
            assert!(output.contains("TERMINDER_DEFAULT_BUILD_OK"), "{output}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "session did not finish: {snapshot:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
