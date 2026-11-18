use crate::{
    bridge::{Bridge, Message, OWNER, Runtime},
    screen::Screen,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use terminder_session_core::{backend, model::*, supervisor::Supervisor};

struct Probe {
    bridge: Bridge,
    screen: Screen,
    snapshot: Option<SessionSnapshot>,
    sequence: u64,
    bytes: u64,
    end: bool,
}
impl Probe {
    fn new(supervisor: Runtime, spec: SpawnSpec) -> anyhow::Result<Self> {
        let screen = Screen::new(spec.size);
        Ok(Self {
            bridge: Bridge::new(supervisor, spec)?,
            screen,
            snapshot: None,
            sequence: 0,
            bytes: 0,
            end: false,
        })
    }
    fn pump(&mut self) -> anyhow::Result<()> {
        for _ in 0..16 {
            let Ok(message) = self.bridge.receiver().try_recv() else {
                break;
            };
            match message {
                Message::Output(bytes) => {
                    for reply in self.screen.feed(&bytes)? {
                        self.bridge.input(reply)?;
                    }
                    self.sequence += 1;
                    self.bytes += bytes.len() as u64;
                    self.bridge.ack(self.sequence, self.bytes)?;
                }
                Message::Event(SessionEvent::State { snapshot }) => {
                    anyhow::ensure!(
                        snapshot.cleanup_error.is_none(),
                        "cleanup error: {:?}",
                        snapshot.cleanup_error
                    );
                    self.snapshot = Some(*snapshot);
                }
                Message::Event(SessionEvent::OutputEnd {
                    sequence,
                    bytes,
                    truncated,
                }) => {
                    anyhow::ensure!(!truncated, "output was truncated");
                    anyhow::ensure!(
                        sequence == self.sequence && bytes == self.bytes,
                        "output-end barrier mismatch"
                    );
                    self.end = true;
                }
                Message::Event(SessionEvent::Error { error }) => return Err(error.into()),
                Message::Error(error) | Message::Notice(error) => anyhow::bail!("{error}"),
            }
        }
        Ok(())
    }
    fn finished(&self) -> bool {
        self.end
            && self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.phase == Phase::Finished)
    }
}
fn wait(
    probes: &mut [&mut Probe],
    condition: impl Fn(&[&mut Probe]) -> bool,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        for probe in &mut *probes {
            probe.pump()?;
        }
        if condition(probes) {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "native probe timed out; states: {:?}",
            probes
                .iter()
                .map(|p| (&p.snapshot, p.screen.text()))
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn fixture_spec(fixture: &Path, mode: &str) -> SpawnSpec {
    SpawnSpec {
        profile_id: "gpui-smoke".into(),
        profile_name: "Isolated fixture".into(),
        program: fixture.to_path_buf(),
        args: vec![mode.into()],
        env: BTreeMap::new(),
        cwd: std::env::current_dir().expect("working directory"),
        size: TerminalSize { cols: 80, rows: 24 },
    }
}
#[derive(Serialize)]
struct Case {
    name: String,
    passed: bool,
    elapsed_ms: u128,
    output_bytes: u64,
    detail: String,
}
#[derive(Serialize)]
struct Report {
    windows_build: Option<u32>,
    gpui: String,
    terminal_engine: String,
    rendering_tested: bool,
    shutdown_joined: bool,
    cases: Vec<Case>,
}
fn case(name: &str, run: impl FnOnce() -> anyhow::Result<(u64, String)>) -> Case {
    let start = Instant::now();
    let result = run();
    let (passed, output_bytes, detail) = match result {
        Ok((bytes, detail)) => (true, bytes, detail),
        Err(error) => (false, 0, format!("{error:#}")),
    };
    println!("{}: {}", name, if passed { "PASS" } else { "FAIL" });
    Case {
        name: name.into(),
        passed,
        elapsed_ms: start.elapsed().as_millis(),
        output_bytes,
        detail,
    }
}
pub fn run(fixture: PathBuf, report_path: PathBuf) -> anyhow::Result<()> {
    let fixture = fixture.canonicalize()?;
    let supervisor = Runtime::new(Supervisor::new(backend::native_factory(), 8)?);
    let mut cases = Vec::new();
    cases.push(case("ConPTY ANSI / Unicode / complete exit tail",||{
        let mut p=Probe::new(supervisor.clone(),fixture_spec(&fixture,"exit"))?;
        wait(&mut [&mut p],|p|p[0].finished())?;
        let text=p.screen.text();
        anyhow::ensure!(text.contains("FIXTURE_READY")&&text.contains("中文")&&text.contains("TAIL_COMPLETE"),"expected terminal text absent");
        anyhow::ensure!(p.snapshot.as_ref().and_then(|s|s.exit_code)==Some(23),"exit code mismatch");
        Ok((p.bytes,"ANSI parsed, split UTF-8 decoded into grid, exit=23, exact consumed output-end barrier".into()))
    }));
    cases.push(case(
        "Two sessions / input isolation / resize / normal exit",
        || {
            let mut a = Probe::new(supervisor.clone(), fixture_spec(&fixture, "echo"))?;
            let mut b = Probe::new(supervisor.clone(), fixture_spec(&fixture, "echo"))?;
            wait(&mut [&mut a, &mut b], |p| {
                p.iter().all(|p| p.screen.text().contains("READY"))
            })?;
            a.bridge.input("ALPHA_中文\r".as_bytes().to_vec())?;
            b.bridge.input(b"BETA_ONLY\r".to_vec())?;
            wait(&mut [&mut a, &mut b], |p| {
                p[0].screen.text().contains("ECHO:ALPHA_中文")
                    && p[1].screen.text().contains("ECHO:BETA_ONLY")
            })?;
            anyhow::ensure!(
                !a.screen.text().contains("BETA_ONLY") && !b.screen.text().contains("ALPHA_"),
                "cross-session output leak"
            );
            let resized = TerminalSize { cols: 96, rows: 28 };
            a.screen.resize(resized)?;
            a.bridge.resize(resized)?;
            let binding = a
                .snapshot
                .as_ref()
                .expect("running snapshot")
                .binding
                .clone();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !supervisor
                .supervisor()
                .list(OWNER)?
                .iter()
                .any(|s| s.binding == binding && s.size == resized)
            {
                anyhow::ensure!(Instant::now() < deadline, "backend resize not observed");
                a.pump()?;
                b.pump()?;
                thread::sleep(Duration::from_millis(5));
            }
            a.bridge.input(b"quit\r".to_vec())?;
            b.bridge.input(b"quit\r".to_vec())?;
            wait(&mut [&mut a, &mut b], |p| p.iter().all(|p| p.finished()))?;
            Ok((
                a.bytes + b.bytes,
                "Independent ConPTYs; Unicode echo; backend resize to 96x28; both completed".into(),
            ))
        },
    ));
    cases.push(case("100000-line flood / bounded consumption / tail",||{
        let mut p=Probe::new(supervisor.clone(),fixture_spec(&fixture,"flood"))?;
        wait(&mut [&mut p],|p|p[0].finished())?;
        anyhow::ensure!(p.screen.text().contains("099999"),"last flood record absent");
        anyhow::ensure!(p.bytes>1_000_000,"flood did not exercise streaming");
        Ok((p.bytes,"100000 records parsed; bounded adapter and core flow-control; complete final record".into()))
    }));
    cases.push(case("Close running session / adapter teardown", || {
        let mut p = Probe::new(supervisor.clone(), fixture_spec(&fixture, "linger"))?;
        wait(&mut [&mut p], |p| {
            p[0].snapshot
                .as_ref()
                .is_some_and(|s| s.phase == Phase::Running)
                && p[0].screen.text().contains("READY")
        })?;
        p.bridge.close();
        wait(&mut [&mut p], |p| p[0].finished())?;
        anyhow::ensure!(
            p.snapshot
                .as_ref()
                .is_some_and(|s| s.reason == Some(FinishReason::ClosedByUser)),
            "unexpected close reason"
        );
        anyhow::ensure!(p.snapshot.as_ref().and_then(|s| s.exit_code) == Some(1), "close returned an unexpected exit code: {:?}", p.snapshot);
        let bytes = p.bytes;
        drop(p);
        Ok((
            bytes,
            "Running process closed; terminal ended; view dropped; adapters joined by final runtime shutdown".into(),
        ))
    }));
    cases.push(case(
        "Real Windows shell / 10 short-process repetitions",
        || {
            let program = PathBuf::from(
                std::env::var_os("SystemRoot")
                    .ok_or_else(|| anyhow::anyhow!("SystemRoot missing"))?,
            )
            .join("System32")
            .join("cmd.exe");
            // cmd inspects its raw command line; a forward slash in argv[0] can
            // be interpreted as a switch. Build the native path by components.
            let mut total = 0;
            for index in 0..10 {
                let marker = format!("GPUI_REAL_CMD_{index}_OK");
                let mut launch = fixture_spec(&program, "unused");
                launch.args = vec![
                    "/D".into(),
                    "/Q".into(),
                    "/C".into(),
                    format!("echo {marker}").into(),
                ];
                let mut p = Probe::new(supervisor.clone(), launch)?;
                wait(&mut [&mut p], |p| p[0].finished())?;
                anyhow::ensure!(
                    p.screen.text().contains(&marker),
                    "real cmd iteration {index} output missing; bytes={}; screen={:?}; state={:?}",
                    p.bytes,
                    p.screen.text(),
                    p.snapshot
                );
                anyhow::ensure!(
                    p.snapshot.as_ref().and_then(|s| s.exit_code) == Some(0),
                    "cmd iteration {index} exit code mismatch; screen={:?}; state={:?}",
                    p.screen.text(),
                    p.snapshot
                );
                total += p.bytes;
            }
            Ok((
                total,
                "10 fresh system cmd.exe processes: expected unique output, complete tail, exit=0"
                    .into(),
            ))
        },
    ));
    cases.push(case("Spawn failure is visible", || {
        let mut launch = fixture_spec(&fixture, "exit");
        launch.program = fixture.with_file_name("terminder-nonexistent-smoke.exe");
        let bridge = Bridge::new(supervisor.clone(), launch)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match bridge.receiver().try_recv() {
                Ok(Message::Error(error)) => return Ok((0, format!("Observed: {error}"))),
                Ok(_) => {}
                Err(_) => {}
            }
            anyhow::ensure!(Instant::now() < deadline, "missing launch error");
            thread::sleep(Duration::from_millis(5));
        }
    }));
    supervisor.begin_shutdown();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !supervisor.poll_shutdown()? {
        anyhow::ensure!(Instant::now() < deadline, "supervisor shutdown timeout");
        thread::sleep(Duration::from_millis(10));
    }
    let passed = cases.iter().all(|c| c.passed);
    let report = Report {
        windows_build: backend::windows_build(),
        gpui: "0.2.2".into(),
        terminal_engine: "alacritty_terminal 0.25.1".into(),
        rendering_tested: false,
        shutdown_joined: true,
        cases,
    };
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    anyhow::ensure!(
        passed,
        "native desktop checks failed; see {}",
        report_path.display()
    );
    Ok(())
}
