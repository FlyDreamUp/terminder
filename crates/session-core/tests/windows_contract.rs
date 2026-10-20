#![cfg(all(windows, feature = "test-fixture"))]
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use terminder_session_core::{
    backend::native_factory,
    model::*,
    supervisor::{Sink, Supervisor},
};

#[derive(Default)]
struct Capture {
    bytes: Mutex<Vec<u8>>,
    counts: Mutex<(u64, u64)>,
    events: Mutex<Vec<SessionEvent>>,
}
impl Sink for Capture {
    fn output(&self, bytes: Vec<u8>) -> Result<()> {
        let mut counts = self.counts.lock().unwrap();
        counts.0 += 1;
        counts.1 += bytes.len() as u64;
        self.bytes.lock().unwrap().extend(bytes);
        Ok(())
    }
    fn event(&self, event: SessionEvent) -> Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}
fn spec(mode: &str) -> SpawnSpec {
    SpawnSpec {
        profile_id: "fixture".into(),
        profile_name: "Fixture".into(),
        program: env!("CARGO_BIN_EXE_pty-fixture").into(),
        args: vec![mode.into()],
        env: BTreeMap::new(),
        cwd: std::env::current_dir().unwrap(),
        cwd_source: "explicit".into(),
        size: TerminalSize {
            cols: 120,
            rows: 40,
        },
    }
}
fn wait(
    s: &Supervisor,
    b: &Binding,
    c: &Capture,
    condition: impl Fn(&SessionSnapshot) -> bool,
    ack: bool,
) -> SessionSnapshot {
    let deadline = Instant::now() + Duration::from_secs(9);
    loop {
        if ack {
            let (seq, bytes) = *c.counts.lock().unwrap();
            s.ack("main", b.clone(), seq, bytes).unwrap();
        }
        let snapshot = s
            .list("main")
            .unwrap()
            .into_iter()
            .find(|v| v.binding == *b)
            .unwrap();
        if condition(&snapshot) {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "timeout: {snapshot:?}");
        thread::sleep(Duration::from_millis(10));
    }
}
fn start(mode: &str) -> (Supervisor, Binding, Arc<Capture>) {
    let s = Supervisor::new(native_factory(), 16).unwrap();
    let h = s.prepare("main", spec(mode)).unwrap();
    let c = Arc::new(Capture::default());
    s.start(
        "main",
        h.binding.clone(),
        TerminalSize {
            cols: 120,
            rows: 40,
        },
        c.clone(),
    )
    .unwrap();
    (s, h.binding, c)
}
#[test]
fn native_exit_preserves_tail_and_reports_exit_code() {
    let (s, b, c) = start("exit");
    let end = wait(&s, &b, &c, |s| s.phase == Phase::Finished, true);
    assert_eq!(end.exit_code, Some(23));
    assert_eq!(end.reason, Some(FinishReason::Exited { code: 23 }));
    assert!(end.tail_complete);
    let text = String::from_utf8(c.bytes.lock().unwrap().clone()).unwrap();
    assert!(text.contains("TAIL_COMPLETE"));
    assert!(text.contains("中文"));
}
#[test]
fn close_unblocks_a_saturated_consumer_without_ack() {
    let (s, b, c) = start("flood");
    wait(&s, &b, &c, |s| s.phase == Phase::Running, false);
    thread::sleep(Duration::from_millis(300));
    let start = Instant::now();
    s.close("main", &b, false).unwrap();
    let end = wait(&s, &b, &c, |s| s.phase == Phase::Finished, false);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(end.reason, Some(FinishReason::ClosedByUser));
    assert!(c.bytes.lock().unwrap().len() <= OUTPUT_WINDOW_BYTES);
}
#[test]
fn real_input_resize_and_old_epoch_are_isolated() {
    let (s, b, c) = start("echo");
    wait(&s, &b, &c, |s| s.phase == Phase::Running, true);
    s.resize("main", b.clone(), 1, TerminalSize { cols: 92, rows: 28 })
        .unwrap();
    let mut stale = b.clone();
    stale.view_epoch = uuid::Uuid::new_v4().to_string();
    assert!(s.input("main", stale, 1, b"bad\r".to_vec()).is_err());
    s.input("main", b.clone(), 1, b"quit\r".to_vec()).unwrap();
    let end = wait(&s, &b, &c, |s| s.phase == Phase::Finished, true);
    assert_eq!(end.exit_code, Some(0));
    let text = String::from_utf8_lossy(&c.bytes.lock().unwrap()).into_owned();
    assert!(text.contains("ECHO:quit"));
    assert!(!text.contains("bad"));
}
#[test]
fn unicode_paths_empty_and_quoted_arguments_and_environment() {
    let dir = tempfile::Builder::new()
        .prefix("terminder 中文 ")
        .tempdir()
        .unwrap();
    let mut spec = spec("metadata");
    spec.cwd = dir.path().to_path_buf();
    spec.args
        .extend(["".into(), "a b".into(), "a\"b\\".into(), "中文".into()]);
    spec.env
        .insert("TERMINDER_FIXTURE".into(), Some("value 中文".into()));
    let s = Supervisor::new(native_factory(), 16).unwrap();
    let h = s.prepare("main", spec).unwrap();
    let c = Arc::new(Capture::default());
    s.start(
        "main",
        h.binding.clone(),
        TerminalSize {
            cols: 200,
            rows: 40,
        },
        c.clone(),
    )
    .unwrap();
    let end = wait(&s, &h.binding, &c, |s| s.phase == Phase::Finished, true);
    assert_eq!(end.exit_code, Some(0));
    let text = String::from_utf8_lossy(&c.bytes.lock().unwrap()).into_owned();
    assert!(
        text.contains("ARGS=[\"\",\"a b\",\"a\\\"b\\\\\",\"中文\"]"),
        "{text}"
    );
    assert!(text.contains("ENV=value 中文"));
    assert!(text.contains("terminder 中文"));
}
#[test]
fn job_owns_descendants_before_the_shell_runs() {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    let (s, b, c) = start("descendant");
    wait(&s, &b, &c, |s| s.phase == Phase::Running, true);
    let deadline = Instant::now() + Duration::from_secs(3);
    let pid = loop {
        let text = String::from_utf8_lossy(&c.bytes.lock().unwrap()).into_owned();
        if let Some(after) = text.split("CHILD_PID=").nth(1) {
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(pid) = digits.parse::<u32>() {
                break pid;
            }
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    };
    let process = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    assert!(!process.is_null());
    s.close("main", &b, false).unwrap();
    wait(&s, &b, &c, |s| s.phase == Phase::Finished, true);
    assert_eq!(unsafe { WaitForSingleObject(process, 0) }, WAIT_OBJECT_0);
    unsafe {
        CloseHandle(process);
    }
}

#[test]
fn close_cancels_a_writer_when_the_program_never_reads_input() {
    let (s, b, c) = start("linger");
    wait(&s, &b, &c, |s| s.phase == Phase::Running, true);
    for sequence in 1..=32 {
        s.input("main", b.clone(), sequence, vec![b'x'; INPUT_CHUNK])
            .unwrap();
    }
    let started = Instant::now();
    s.close("main", &b, false).unwrap();
    wait(&s, &b, &c, |s| s.phase == Phase::Finished, false);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn abrupt_host_death_closes_the_job_and_kills_root_and_descendant() {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    struct Host(std::process::Child);
    impl Drop for Host {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("owned-pids.json");
    let mut host = Host(
        std::process::Command::new(env!("CARGO_BIN_EXE_pty-fixture"))
            .arg("job-host")
            .arg(&pid_file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let pids: [u32; 2] = loop {
        if let Ok(bytes) = std::fs::read(&pid_file)
            && let Ok(pids) = serde_json::from_slice(&bytes)
        {
            break pids;
        }
        assert!(
            host.0.try_wait().unwrap().is_none(),
            "fixture host exited early"
        );
        assert!(
            Instant::now() < deadline,
            "job members did not publish their identities"
        );
        thread::sleep(Duration::from_millis(10));
    };
    let processes: Vec<_> = pids
        .into_iter()
        .map(|pid| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) })
        .collect();
    assert!(processes.iter().all(|p| !p.is_null()));
    host.0.kill().unwrap();
    host.0.wait().unwrap();
    let outcomes: Vec<_> = processes
        .into_iter()
        .map(|process| unsafe {
            let result = WaitForSingleObject(process, 5000);
            CloseHandle(process);
            result
        })
        .collect();
    assert_eq!(outcomes, vec![WAIT_OBJECT_0, WAIT_OBJECT_0]);
}

#[test]
fn repeated_create_close_leaves_no_worker_growth() {
    use windows_sys::Win32::System::Threading::*;
    let before_types = handle_types();
    let mut before = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut before) },
        0
    );
    for _ in 0..12 {
        let (s, b, c) = start("echo");
        wait(&s, &b, &c, |s| s.phase == Phase::Running, true);
        s.close("main", &b, false).unwrap();
        wait(&s, &b, &c, |s| s.phase == Phase::Finished, true);
        s.shutdown();
        while !s.is_stopped() {
            thread::sleep(Duration::from_millis(5));
        }
        s.join_if_stopped().unwrap();
    }
    let mut after = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut after) },
        0
    );
    assert!(
        after <= before + 4,
        "handle growth {before} -> {after}: {before_types:?} -> {:?}",
        handle_types()
    );
}

fn handle_types() -> BTreeMap<String, usize> {
    use windows_sys::Win32::System::{
        Diagnostics::ProcessSnapshotting::*,
        Threading::{GetCurrentProcess, QueryFullProcessImageNameW},
    };
    let mut snapshot = std::ptr::null_mut();
    let mut marker = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            PssCaptureSnapshot(
                GetCurrentProcess(),
                PSS_CAPTURE_HANDLES | PSS_CAPTURE_HANDLE_NAME_INFORMATION,
                0,
                &mut snapshot,
            )
        },
        0
    );
    assert_eq!(
        unsafe { PssWalkMarkerCreate(std::ptr::null(), &mut marker) },
        0
    );
    let mut counts = BTreeMap::new();
    let mut entry = PSS_HANDLE_ENTRY::default();
    while unsafe {
        PssWalkSnapshot(
            snapshot,
            PSS_WALK_HANDLES,
            marker,
            (&mut entry as *mut PSS_HANDLE_ENTRY).cast(),
            std::mem::size_of::<PSS_HANDLE_ENTRY>() as u32,
        )
    } == 0
    {
        if !entry.TypeName.is_null() {
            let name = unsafe {
                String::from_utf16_lossy(std::slice::from_raw_parts(
                    entry.TypeName,
                    entry.TypeNameLength as usize / 2,
                ))
            };
            let name = if name.trim_end_matches('\0') == "Process" {
                let mut length = 1024_u32;
                let mut path = vec![0_u16; length as usize];
                let image = if unsafe {
                    QueryFullProcessImageNameW(entry.Handle, 0, path.as_mut_ptr(), &mut length)
                } != 0
                {
                    String::from_utf16_lossy(&path[..length as usize])
                        .rsplit('\\')
                        .next()
                        .unwrap_or("unknown")
                        .to_owned()
                } else {
                    "unknown".into()
                };
                format!("Process({image})")
            } else {
                name
            };
            *counts.entry(name).or_insert(0) += 1;
        }
    }
    unsafe {
        PssWalkMarkerFree(marker);
        PssFreeSnapshot(GetCurrentProcess(), snapshot);
    }
    counts
}
