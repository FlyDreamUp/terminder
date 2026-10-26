use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use terminder_session_core::{
    backend::{Backend, Factory, IoState, read_loop},
    model::*,
    supervisor::{Sink, Supervisor},
    transport::GlobalBudget,
};

#[derive(Default)]
struct Events(Mutex<Vec<SessionEvent>>);
impl Sink for Events {
    fn output(&self, _: Vec<u8>) -> Result<()> {
        Ok(())
    }
    fn event(&self, event: SessionEvent) -> Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}
struct Controlled {
    io: Arc<IoState>,
    closed: bool,
    exited: Arc<AtomicBool>,
    reaped: Arc<AtomicUsize>,
}
impl Backend for Controlled {
    fn io(&self) -> Arc<IoState> {
        self.io.clone()
    }
    fn resize(&mut self, size: TerminalSize) -> Result<()> {
        size.validate().map(|_| ())
    }
    fn poll_exit(&mut self) -> Result<Option<u32>> {
        Ok((self.closed || self.exited.load(Ordering::Acquire)).then_some(23))
    }
    fn begin_close(&mut self) -> Result<()> {
        self.closed = true;
        self.io.drain();
        Ok(())
    }
    fn force_kill(&mut self) -> Result<()> {
        self.closed = true;
        Ok(())
    }
    fn cancel_io(&self) {
        self.io.stop();
    }
    fn try_reap(&mut self) -> Result<bool> {
        Ok(self.closed)
    }
}
impl Drop for Controlled {
    fn drop(&mut self) {
        self.reaped.fetch_add(1, Ordering::AcqRel);
    }
}
fn factory(delay: Duration, exited: Arc<AtomicBool>, reaped: Arc<AtomicUsize>) -> Factory {
    Arc::new(move |_, budget| {
        thread::sleep(delay);
        let io = Arc::new(IoState::default());
        read_loop(std::io::Cursor::new(Vec::<u8>::new()), io.clone(), budget);
        Ok(Box::new(Controlled {
            io,
            closed: false,
            exited: exited.clone(),
            reaped: reaped.clone(),
        }))
    })
}
fn spec() -> SpawnSpec {
    SpawnSpec {
        profile_id: "fixture".into(),
        profile_name: "Fixture".into(),
        program: std::env::current_exe().unwrap(),
        args: vec![],
        env: BTreeMap::new(),
        cwd: std::env::current_dir().unwrap(),
        cwd_source: "explicit".into(),
        size: TerminalSize::default(),
    }
}
fn wait(supervisor: &Supervisor, binding: &Binding, phase: Phase) -> SessionSnapshot {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(s) = supervisor
            .list("main")
            .unwrap()
            .into_iter()
            .find(|s| s.binding == *binding)
            && s.phase == phase
        {
            return s;
        }
        assert!(Instant::now() < deadline, "state did not reach {phase:?}");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn close_while_starting_reaps_late_result_and_never_publishes_running() {
    let reaped = Arc::new(AtomicUsize::new(0));
    let s = Supervisor::new(
        factory(
            Duration::from_millis(100),
            Arc::new(AtomicBool::new(false)),
            reaped.clone(),
        ),
        16,
    )
    .unwrap();
    let h = s.prepare("main", spec()).unwrap();
    let events = Arc::new(Events::default());
    s.start(
        "main",
        h.binding.clone(),
        TerminalSize::default(),
        events.clone(),
    )
    .unwrap();
    h.close();
    h.close();
    let end = wait(&s, &h.binding, Phase::Finished);
    assert_eq!(end.reason, Some(FinishReason::ClosedByUser));
    assert_eq!(reaped.load(Ordering::Acquire), 1);
    let events = events.0.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e,SessionEvent::State{snapshot} if snapshot.phase==Phase::Running))
    );
    assert_eq!(
        events
            .iter()
            .filter(
                |e| matches!(e,SessionEvent::State{snapshot} if snapshot.phase==Phase::Finished)
            )
            .count(),
        1
    );
}
#[test]
fn bindings_limits_resize_and_close_are_authoritative() {
    let s = Supervisor::new(
        factory(
            Duration::ZERO,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(0)),
        ),
        1,
    )
    .unwrap();
    let h = s.prepare("main", spec()).unwrap();
    assert_eq!(
        s.prepare("main", spec()).err().unwrap().code,
        "session_limit"
    );
    assert!(
        s.start(
            "other",
            h.binding.clone(),
            TerminalSize::default(),
            Arc::new(Events::default())
        )
        .is_err()
    );
    s.start(
        "main",
        h.binding.clone(),
        TerminalSize::default(),
        Arc::new(Events::default()),
    )
    .unwrap();
    wait(&s, &h.binding, Phase::Running);
    let mut stale = h.binding.clone();
    stale.view_epoch = uuid::Uuid::new_v4().to_string();
    assert!(s.input("main", stale, 1, b"a".to_vec()).is_err());
    s.input("main", h.binding.clone(), 1, b"a".to_vec())
        .unwrap();
    s.input("main", h.binding.clone(), 1, b"a".to_vec())
        .unwrap();
    assert!(
        s.input("main", h.binding.clone(), 3, b"b".to_vec())
            .is_err()
    );
    s.resize(
        "main",
        h.binding.clone(),
        2,
        TerminalSize {
            cols: 100,
            rows: 50,
        },
    )
    .unwrap();
    s.resize(
        "main",
        h.binding.clone(),
        1,
        TerminalSize { cols: 80, rows: 24 },
    )
    .unwrap();
    assert_eq!(
        s.list("main").unwrap()[0].size,
        TerminalSize {
            cols: 100,
            rows: 50
        }
    );
    h.close();
    assert!(
        s.input("main", h.binding.clone(), 2, b"b".to_vec())
            .is_err()
    );
    wait(&s, &h.binding, Phase::Finished);
    s.forget("main", h.binding).unwrap();
    assert!(s.list("main").unwrap().is_empty());
}
#[test]
fn exit_and_close_race_emits_only_one_final_result() {
    let exited = Arc::new(AtomicBool::new(false));
    let s = Supervisor::new(
        factory(
            Duration::ZERO,
            exited.clone(),
            Arc::new(AtomicUsize::new(0)),
        ),
        16,
    )
    .unwrap();
    let h = s.prepare("main", spec()).unwrap();
    let events = Arc::new(Events::default());
    s.start(
        "main",
        h.binding.clone(),
        TerminalSize::default(),
        events.clone(),
    )
    .unwrap();
    wait(&s, &h.binding, Phase::Running);
    exited.store(true, Ordering::Release);
    h.close();
    wait(&s, &h.binding, Phase::Finished);
    assert_eq!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(
                |e| matches!(e,SessionEvent::State{snapshot} if snapshot.phase==Phase::Finished)
            )
            .count(),
        1
    );
}
#[test]
fn bounded_input_rejects_before_accepting_extra_data() {
    let io = IoState::default();
    for _ in 0..32 {
        io.enqueue_input(vec![0; INPUT_CHUNK]).unwrap();
    }
    assert_eq!(io.input_bytes(), INPUT_BUDGET);
    assert!(io.enqueue_input(vec![1]).is_err());
    assert_eq!(io.input_bytes(), INPUT_BUDGET);
}
#[test]
fn read_loop_keeps_utf8_split_bytes_without_text_conversion() {
    let io = Arc::new(IoState::default());
    let budget = Arc::new(GlobalBudget::default());
    let bytes = vec![
        0xe4, 0xb8, 0xad, 0xf0, 0x9f, 0x98, 0x80, 0x1b, b'[', b'2', b'J',
    ];
    read_loop(
        std::io::Cursor::new(bytes.clone()),
        io.clone(),
        budget.clone(),
    );
    assert_eq!(io.pop_output().unwrap().bytes, bytes);
    assert!(io.output_done());
    assert_eq!(budget.used(), 0);
}

#[test]
fn paste_reservation_protects_the_entire_payload_before_any_chunk_is_accepted() {
    let io = IoState::default();
    for _ in 0..24 {
        io.enqueue_input(vec![0; INPUT_CHUNK]).unwrap();
    }
    assert!(io.reserve_input(256 * 1024).is_err());
    assert_eq!(io.input_bytes(), 384 * 1024);
    let token = io.reserve_input(128 * 1024).unwrap();
    assert!(io.enqueue_input(vec![1]).is_err());
    for _ in 0..8 {
        io.enqueue_reserved(vec![2; INPUT_CHUNK], Some(&token))
            .unwrap();
    }
    assert_eq!(io.input_bytes(), INPUT_BUDGET);
    assert!(io.enqueue_reserved(vec![3], Some(&token)).is_err());
}
#[test]
fn reservations_cannot_be_used_by_another_session_or_after_close() {
    let first = IoState::default();
    let second = IoState::default();
    let token = first.reserve_input(10).unwrap();
    assert!(second.enqueue_reserved(vec![1; 10], Some(&token)).is_err());
    assert_eq!(second.input_bytes(), 0);
    first.drain();
    assert!(first.enqueue_reserved(vec![1; 10], Some(&token)).is_err());
}

#[test]
fn lost_document_reclaims_prepared_running_and_finished_sessions() {
    let reaped = Arc::new(AtomicUsize::new(0));
    let s = Supervisor::new(
        factory(
            Duration::ZERO,
            Arc::new(AtomicBool::new(false)),
            reaped.clone(),
        ),
        1,
    )
    .unwrap();
    for phase in [Phase::Preparing, Phase::Running, Phase::Finished] {
        let h = s.prepare("main", spec()).unwrap();
        if phase != Phase::Preparing {
            s.start(
                "main",
                h.binding.clone(),
                TerminalSize::default(),
                Arc::new(Events::default()),
            )
            .unwrap();
            wait(&s, &h.binding, Phase::Running);
        }
        if phase == Phase::Finished {
            h.close();
            wait(&s, &h.binding, Phase::Finished);
        }
        s.view_lost("main");
        let deadline = Instant::now() + Duration::from_secs(4);
        while !s.list("main").unwrap().is_empty() {
            assert!(
                Instant::now() < deadline,
                "detached session metadata was retained"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            s.close("main", &h.binding, false).unwrap_err().code,
            "session_missing"
        );
    }
    assert_eq!(reaped.load(Ordering::Acquire), 2);
}

#[test]
fn failed_output_is_not_retried_and_releases_the_global_budget() {
    struct FailedOutput(AtomicUsize);
    impl Sink for FailedOutput {
        fn output(&self, _: Vec<u8>) -> Result<()> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Err(CoreError::new("channel_failed", "output channel lost"))
        }
        fn event(&self, _: SessionEvent) -> Result<()> {
            Ok(())
        }
    }
    let budget = Arc::new(Mutex::new(None));
    let observed = budget.clone();
    let factory: Factory = Arc::new(move |_, budget| {
        *observed.lock().unwrap() = Some(budget.clone());
        let io = Arc::new(IoState::default());
        read_loop(
            std::io::Cursor::new(vec![b'x'; 2 * OUTPUT_CHUNK]),
            io.clone(),
            budget,
        );
        Ok(Box::new(Controlled {
            io,
            closed: false,
            exited: Arc::new(AtomicBool::new(false)),
            reaped: Arc::new(AtomicUsize::new(0)),
        }))
    });
    let supervisor = Supervisor::new(factory, 1).unwrap();
    let handle = supervisor.prepare("main", spec()).unwrap();
    let sink = Arc::new(FailedOutput(AtomicUsize::new(0)));
    supervisor
        .start(
            "main",
            handle.binding.clone(),
            TerminalSize::default(),
            sink.clone(),
        )
        .unwrap();
    let end = wait(&supervisor, &handle.binding, Phase::Finished);
    assert_eq!(end.reason, Some(FinishReason::ViewLost));
    assert!(!end.tail_complete);
    assert_eq!(sink.0.load(Ordering::Acquire), 1);
    assert_eq!(budget.lock().unwrap().as_ref().unwrap().used(), 0);
    // Metadata stays available until the user removes the tab; it must not retain byte leases.
    assert_eq!(supervisor.list("main").unwrap().len(), 1);
}

#[test]
fn losing_a_finished_view_releases_unacknowledged_output() {
    let budget = Arc::new(Mutex::new(None));
    let observed = budget.clone();
    let factory: Factory = Arc::new(move |_, budget| {
        *observed.lock().unwrap() = Some(budget.clone());
        let io = Arc::new(IoState::default());
        read_loop(std::io::Cursor::new(b"tail"), io.clone(), budget);
        Ok(Box::new(Controlled {
            io,
            closed: false,
            exited: Arc::new(AtomicBool::new(true)),
            reaped: Arc::new(AtomicUsize::new(0)),
        }))
    });
    let supervisor = Supervisor::new(factory, 1).unwrap();
    let handle = supervisor.prepare("main", spec()).unwrap();
    supervisor
        .start(
            "main",
            handle.binding.clone(),
            TerminalSize::default(),
            Arc::new(Events::default()),
        )
        .unwrap();
    wait(&supervisor, &handle.binding, Phase::Finished);
    let budget = budget.lock().unwrap().as_ref().unwrap().clone();
    assert_eq!(budget.used(), 4);
    handle.view_failed();
    let deadline = Instant::now() + Duration::from_secs(1);
    while budget.used() != 0 {
        assert!(
            Instant::now() < deadline,
            "finished view retained its output budget"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
