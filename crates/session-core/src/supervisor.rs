use crate::{
    backend::{Backend, Factory},
    model::*,
    transport::{GlobalBudget, InputOrder, OutputFlow},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub trait Sink: Send + Sync {
    fn output(&self, bytes: Vec<u8>) -> Result<()>;
    fn event(&self, event: SessionEvent) -> Result<()>;
}
#[derive(Clone)]
pub struct SessionHandle {
    pub binding: Binding,
    control: Arc<Control>,
}
struct Control {
    close: AtomicBool,
    view_failed: AtomicBool,
    detached: AtomicBool,
}
impl SessionHandle {
    pub fn close(&self) {
        self.control.close.store(true, Ordering::Release);
    }
    pub fn view_failed(&self) {
        self.control.view_failed.store(true, Ordering::Release);
    }
}
struct Entry {
    owner: String,
    spec: SpawnSpec,
    snapshot: SessionSnapshot,
    control: Arc<Control>,
    sink: Option<Arc<dyn Sink>>,
    backend: Option<Box<dyn Backend>>,
    launch: Option<JoinHandle<Result<Box<dyn Backend>>>>,
    flow: OutputFlow,
    input: InputOrder,
    revision: u64,
    since: Instant,
    closing_since: Option<Instant>,
    output_ended: bool,
    killed: bool,
}
impl Entry {
    fn authorize(&self, owner: &str, binding: &Binding) -> Result<()> {
        binding.validate()?;
        if self.owner != owner || self.snapshot.binding != *binding {
            return Err(CoreError::new("wrong_binding", "窗口或会话绑定已失效"));
        }
        Ok(())
    }
    fn event(&self, event: SessionEvent) {
        if let Some(sink) = &self.sink
            && sink.event(event).is_err()
        {
            self.control.view_failed.store(true, Ordering::Release);
        }
    }
    fn publish(&self) {
        self.event(SessionEvent::State {
            snapshot: Box::new(self.snapshot.clone()),
        });
    }
    fn phase(&mut self, phase: Phase) {
        tracing::info!(session_id=%self.snapshot.binding.session_id, phase=?phase, "session_transition");
        self.snapshot.phase = phase;
        self.since = Instant::now();
        self.publish();
    }
    fn error(&self, error: CoreError) {
        self.event(SessionEvent::Error { error });
    }
    fn close(&mut self, reason: FinishReason) {
        if self.snapshot.phase == Phase::Finished || self.closing_since.is_some() {
            return;
        }
        self.snapshot.reason = Some(reason);
        self.closing_since = Some(Instant::now());
        self.phase(Phase::Closing);
        if let Some(backend) = &mut self.backend {
            backend.io().drain();
            if let Err(error) = backend.begin_close() {
                self.snapshot.cleanup_error = Some(error.clone());
                self.error(error);
            }
        }
        self.phase(Phase::Finalizing);
    }
    fn end_output(&mut self) {
        if self.output_ended {
            return;
        }
        let (sequence, bytes) = self.flow.end();
        self.event(SessionEvent::OutputEnd {
            sequence: sequence.to_string(),
            bytes: bytes.to_string(),
            truncated: !self.snapshot.tail_complete,
        });
        self.output_ended = true;
    }
    fn finished(&mut self) {
        self.end_output();
        self.phase(Phase::Finished);
        // Finished sessions retain only metadata. The front-end keeps its terminal buffer.
    }
}

type Reply<T> = SyncSender<Result<T>>;
enum Command {
    Prepare {
        owner: String,
        spec: SpawnSpec,
        reply: Reply<SessionHandle>,
    },
    Start {
        owner: String,
        binding: Binding,
        size: TerminalSize,
        sink: Arc<dyn Sink>,
        reply: Reply<()>,
    },
    Input {
        owner: String,
        binding: Binding,
        sequence: u64,
        bytes: Vec<u8>,
        reservation: Option<String>,
        reply: Reply<()>,
    },
    Reserve {
        owner: String,
        binding: Binding,
        bytes: usize,
        reply: Reply<String>,
    },
    Ack {
        owner: String,
        binding: Binding,
        sequence: u64,
        bytes: u64,
        reply: Reply<()>,
    },
    Resize {
        owner: String,
        binding: Binding,
        revision: u64,
        size: TerminalSize,
        reply: Reply<()>,
    },
    List {
        owner: String,
        reply: Reply<Vec<SessionSnapshot>>,
    },
    Forget {
        owner: String,
        binding: Binding,
        reply: Reply<()>,
    },
    Limit {
        max: usize,
        reply: Reply<()>,
    },
}
struct Shared {
    sender: SyncSender<Command>,
    shutdown: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    handles: Arc<Mutex<BTreeMap<String, (String, SessionHandle)>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
#[derive(Clone)]
pub struct Supervisor(Arc<Shared>);
impl Supervisor {
    pub fn new(factory: Factory, max_sessions: usize) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let done = stopped.clone();
        let handles = Arc::new(Mutex::new(BTreeMap::new()));
        let controls = handles.clone();
        let worker = thread::Builder::new()
            .name("session-supervisor".into())
            .spawn(move || {
                run(
                    receiver,
                    factory,
                    max_sessions.clamp(1, MAX_SESSIONS),
                    stop,
                    controls,
                );
                done.store(true, Ordering::Release);
            })
            .map_err(|e| CoreError::io("supervisor_start", e))?;
        Ok(Self(Arc::new(Shared {
            sender,
            shutdown,
            stopped,
            handles,
            worker: Mutex::new(Some(worker)),
        })))
    }
    fn request<T>(&self, build: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        if self.0.shutdown.load(Ordering::Acquire) {
            return Err(CoreError::new("shutting_down", "应用正在结束会话"));
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.0
            .sender
            .try_send(build(tx))
            .map_err(|_| CoreError::new("control_busy", "控制请求繁忙，请稍后重试"))?;
        rx.recv_timeout(Duration::from_secs(3)).map_err(|_| {
            CoreError::new(
                "control_timeout",
                "请求结果未知，请查询会话状态；不要重发输入",
            )
        })?
    }
    pub fn prepare(&self, owner: &str, spec: SpawnSpec) -> Result<SessionHandle> {
        let handle = self.request(|reply| Command::Prepare {
            owner: owner.into(),
            spec,
            reply,
        })?;
        self.0.handles.lock().unwrap().insert(
            handle.binding.session_id.clone(),
            (owner.into(), handle.clone()),
        );
        Ok(handle)
    }
    pub fn start(
        &self,
        owner: &str,
        binding: Binding,
        size: TerminalSize,
        sink: Arc<dyn Sink>,
    ) -> Result<()> {
        self.request(|reply| Command::Start {
            owner: owner.into(),
            binding,
            size,
            sink,
            reply,
        })
    }
    pub fn input(
        &self,
        owner: &str,
        binding: Binding,
        sequence: u64,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.input_reserved(owner, binding, sequence, bytes, None)
    }
    pub fn input_reserved(
        &self,
        owner: &str,
        binding: Binding,
        sequence: u64,
        bytes: Vec<u8>,
        reservation: Option<String>,
    ) -> Result<()> {
        if bytes.len() > INPUT_CHUNK
            || reservation
                .as_ref()
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_err())
        {
            return Err(CoreError::new("input_size", "输入块或预约标识无效"));
        }
        self.request(|reply| Command::Input {
            owner: owner.into(),
            binding,
            sequence,
            bytes,
            reservation,
            reply,
        })
    }
    pub fn reserve_input(&self, owner: &str, binding: Binding, bytes: usize) -> Result<String> {
        self.request(|reply| Command::Reserve {
            owner: owner.into(),
            binding,
            bytes,
            reply,
        })
    }
    pub fn ack(&self, owner: &str, binding: Binding, sequence: u64, bytes: u64) -> Result<()> {
        self.request(|reply| Command::Ack {
            owner: owner.into(),
            binding,
            sequence,
            bytes,
            reply,
        })
    }
    pub fn resize(
        &self,
        owner: &str,
        binding: Binding,
        revision: u64,
        size: TerminalSize,
    ) -> Result<()> {
        self.request(|reply| Command::Resize {
            owner: owner.into(),
            binding,
            revision,
            size,
            reply,
        })
    }
    pub fn list(&self, owner: &str) -> Result<Vec<SessionSnapshot>> {
        self.request(|reply| Command::List {
            owner: owner.into(),
            reply,
        })
    }
    pub fn set_limit(&self, max: usize) -> Result<()> {
        self.request(|reply| Command::Limit { max, reply })
    }
    pub fn close(&self, owner: &str, binding: &Binding, view_failed: bool) -> Result<()> {
        binding.validate()?;
        let handles = self.0.handles.lock().unwrap();
        let (actual, handle) = handles
            .get(&binding.session_id)
            .ok_or_else(|| CoreError::new("session_missing", "会话不存在"))?;
        if actual != owner || handle.binding != *binding {
            return Err(CoreError::new("wrong_binding", "会话绑定无效"));
        }
        if view_failed {
            handle.view_failed();
        } else {
            handle.close();
        }
        Ok(())
    }
    pub fn forget(&self, owner: &str, binding: Binding) -> Result<()> {
        self.request(|reply| Command::Forget {
            owner: owner.into(),
            binding: binding.clone(),
            reply,
        })?;
        self.0.handles.lock().unwrap().remove(&binding.session_id);
        Ok(())
    }
    /// Called only by the trusted host on a definitive native view failure or navigation.
    pub fn view_lost(&self, owner: &str) {
        for (actual, handle) in self.0.handles.lock().unwrap().values() {
            if actual == owner {
                handle.control.detached.store(true, Ordering::Release);
                handle.view_failed();
            }
        }
    }
    pub fn shutdown(&self) {
        self.0.shutdown.store(true, Ordering::Release);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    pub fn join_if_stopped(&self) -> Result<bool> {
        if !self.is_stopped() {
            return Ok(false);
        }
        if let Some(worker) = self.0.worker.lock().unwrap().take() {
            worker
                .join()
                .map_err(|_| CoreError::new("supervisor_panic", "会话调度器异常退出"))?;
        }
        Ok(true)
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // Normal application exit explicitly joins first. Last-owner fallback still owns cleanup.
        if let Some(worker) = self.worker.get_mut().unwrap().take() {
            let _ = worker.join();
        }
    }
}

fn get_entry<'a>(
    entries: &'a mut BTreeMap<String, Entry>,
    owner: &str,
    binding: &Binding,
) -> Result<&'a mut Entry> {
    let entry = entries
        .get_mut(&binding.session_id)
        .ok_or_else(|| CoreError::new("session_missing", "会话不存在"))?;
    entry.authorize(owner, binding)?;
    Ok(entry)
}
fn handle(
    command: Command,
    entries: &mut BTreeMap<String, Entry>,
    factory: &Factory,
    budget: &Arc<GlobalBudget>,
    limit: &mut usize,
) {
    match command {
        Command::Prepare { owner, spec, reply } => {
            let result = (|| {
                spec.validate()?;
                if entries
                    .values()
                    .filter(|e| e.snapshot.phase != Phase::Finished)
                    .count()
                    >= *limit
                    || entries.len() >= 64
                {
                    return Err(CoreError::new(
                        "session_limit",
                        "会话数量已达到上限，请先关闭标签",
                    ));
                }
                let binding = Binding::fresh();
                let control = Arc::new(Control {
                    close: AtomicBool::new(false),
                    view_failed: AtomicBool::new(false),
                    detached: AtomicBool::new(false),
                });
                let handle = SessionHandle {
                    binding: binding.clone(),
                    control: control.clone(),
                };
                let snapshot = SessionSnapshot {
                    binding: binding.clone(),
                    profile_id: spec.profile_id.clone(),
                    profile_name: spec.profile_name.clone(),
                    startup_cwd: spec.cwd.to_str().map(str::to_owned),
                    cwd_source: spec.cwd_source.clone(),
                    phase: Phase::Preparing,
                    reason: None,
                    exit_code: None,
                    tail_complete: true,
                    congested: false,
                    cleanup_error: None,
                    size: spec.size,
                };
                entries.insert(
                    binding.session_id,
                    Entry {
                        owner,
                        spec,
                        snapshot,
                        control,
                        sink: None,
                        backend: None,
                        launch: None,
                        flow: OutputFlow::default(),
                        input: InputOrder::default(),
                        revision: 0,
                        since: Instant::now(),
                        closing_since: None,
                        output_ended: false,
                        killed: false,
                    },
                );
                Ok(handle)
            })();
            let _ = reply.send(result);
        }
        Command::Start {
            owner,
            binding,
            size,
            sink,
            reply,
        } => {
            let result = (|| {
                let entry = get_entry(entries, &owner, &binding)?;
                if entry.snapshot.phase != Phase::Preparing
                    || entry.control.close.load(Ordering::Acquire)
                    || entry.control.view_failed.load(Ordering::Acquire)
                {
                    return Err(CoreError::new("invalid_state", "会话未处于可启动状态"));
                }
                entry.spec.size = size.validate()?;
                entry.snapshot.size = size;
                entry.sink = Some(sink);
                let factory = factory.clone();
                let budget = budget.clone();
                let spec = entry.spec.clone();
                entry.launch = Some(
                    thread::Builder::new()
                        .name("session-create".into())
                        .spawn(move || factory(spec, budget))
                        .map_err(|e| CoreError::io("spawn_worker", e))?,
                );
                entry.phase(Phase::Starting);
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Command::Input {
            owner,
            binding,
            sequence,
            bytes,
            reservation,
            reply,
        } => {
            let result = (|| {
                let e = get_entry(entries, &owner, &binding)?;
                if e.snapshot.phase != Phase::Running
                    || e.control.close.load(Ordering::Acquire)
                    || e.control.view_failed.load(Ordering::Acquire)
                {
                    return Err(CoreError::new("not_running", "会话不接受输入"));
                }
                if !e.input.check(sequence, &bytes)? {
                    return Ok(());
                }
                e.backend
                    .as_ref()
                    .expect("running backend")
                    .io()
                    .enqueue_reserved(bytes.clone(), reservation.as_deref())?;
                e.input.accepted(sequence, &bytes);
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Command::Reserve {
            owner,
            binding,
            bytes,
            reply,
        } => {
            let result = get_entry(entries, &owner, &binding).and_then(|e| {
                if e.snapshot.phase != Phase::Running
                    || e.control.close.load(Ordering::Acquire)
                    || e.control.view_failed.load(Ordering::Acquire)
                {
                    return Err(CoreError::new("not_running", "会话不接受输入"));
                }
                e.backend
                    .as_ref()
                    .expect("running backend")
                    .io()
                    .reserve_input(bytes)
            });
            let _ = reply.send(result);
        }
        Command::Ack {
            owner,
            binding,
            sequence,
            bytes,
            reply,
        } => {
            let result = get_entry(entries, &owner, &binding).and_then(|e| {
                let result = e.flow.ack(sequence, bytes);
                if let Some(b) = &e.backend {
                    b.io().wake();
                }
                result
            });
            let _ = reply.send(result);
        }
        Command::Resize {
            owner,
            binding,
            revision,
            size,
            reply,
        } => {
            let result = (|| {
                let e = get_entry(entries, &owner, &binding)?;
                let size = size.validate()?;
                if revision <= e.revision {
                    return Ok(());
                }
                if e.snapshot.phase != Phase::Running {
                    return Err(CoreError::new("not_running", "会话尚未运行或已结束"));
                }
                e.backend.as_mut().expect("running backend").resize(size)?;
                e.revision = revision;
                e.snapshot.size = size;
                e.publish();
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Command::List { owner, reply } => {
            let _ = reply.send(Ok(entries
                .values()
                .filter(|e| e.owner == owner)
                .map(|e| e.snapshot.clone())
                .collect()));
        }
        Command::Forget {
            owner,
            binding,
            reply,
        } => {
            let result = get_entry(entries, &owner, &binding).and_then(|e| {
                if e.snapshot.phase == Phase::Finished {
                    Ok(())
                } else {
                    Err(CoreError::new(
                        "cleanup_pending",
                        "资源尚未回收，不能移除会话",
                    ))
                }
            });
            if result.is_ok() {
                entries.remove(&binding.session_id);
            }
            let _ = reply.send(result);
        }
        Command::Limit { max, reply } => {
            let result = if (1..=MAX_SESSIONS).contains(&max) {
                *limit = max;
                Ok(())
            } else {
                Err(CoreError::new("session_limit", "会话上限须为 1–16"))
            };
            let _ = reply.send(result);
        }
    }
}

fn run(
    receiver: Receiver<Command>,
    factory: Factory,
    mut limit: usize,
    shutdown: Arc<AtomicBool>,
    controls: Arc<Mutex<BTreeMap<String, (String, SessionHandle)>>>,
) {
    let mut entries = BTreeMap::new();
    let budget = Arc::new(GlobalBudget::default());
    let mut rotation = 0;
    loop {
        match receiver.recv_timeout(Duration::from_millis(4)) {
            Ok(command) => {
                handle(command, &mut entries, &factory, &budget, &mut limit);
                for _ in 0..15 {
                    match receiver.try_recv() {
                        Ok(cmd) => handle(cmd, &mut entries, &factory, &budget, &mut limit),
                        Err(_) => break,
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => shutdown.store(true, Ordering::Release),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let keys: Vec<_> = entries.keys().cloned().collect();
        for index in 0..keys.len() {
            let key = &keys[(index + rotation) % keys.len()];
            tick(
                entries.get_mut(key).expect("key exists"),
                shutdown.load(Ordering::Acquire),
            );
        }
        let detached: Vec<_> = entries
            .iter()
            .filter(|(_, e)| {
                e.snapshot.phase == Phase::Finished
                    && (e.control.detached.load(Ordering::Acquire)
                        || (e.sink.is_none() && e.since.elapsed() > Duration::from_secs(15)))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in detached {
            entries.remove(&id);
            controls.lock().unwrap().remove(&id);
        }
        rotation = rotation.wrapping_add(1);
        if shutdown.load(Ordering::Acquire)
            && entries
                .values()
                .all(|e| e.snapshot.phase == Phase::Finished)
        {
            break;
        }
    }
}

fn tick(e: &mut Entry, shutdown: bool) {
    if e.control.view_failed.load(Ordering::Acquire) {
        // A broken binding cannot return its leases through ACKs, even if its
        // process already finished. Retained tab metadata must not exhaust the budget.
        e.flow.abandon();
    }
    if e.snapshot.phase == Phase::Finished {
        return;
    }
    if e.control.view_failed.load(Ordering::Acquire) {
        e.snapshot.tail_complete = false;
        e.close(FinishReason::ViewLost);
    } else if shutdown || e.control.close.load(Ordering::Acquire) {
        e.close(FinishReason::ClosedByUser);
    }
    if e.snapshot.phase == Phase::Preparing && e.since.elapsed() >= Duration::from_secs(5) {
        e.error(CoreError::new("prepare_timeout", "终端视图未在期限内就绪"));
        e.close(FinishReason::TimedOut);
    }
    if e.snapshot.phase == Phase::Starting && e.since.elapsed() >= Duration::from_secs(10) {
        e.error(CoreError::new(
            "start_timeout",
            "进程创建超时，正在清理迟到的结果",
        ));
        e.close(FinishReason::TimedOut);
    }
    if e.launch.as_ref().is_some_and(JoinHandle::is_finished) {
        match e.launch.take().unwrap().join() {
            Ok(Ok(mut backend)) => {
                if e.closing_since.is_some() {
                    backend.io().drain();
                    if let Err(error) = backend.begin_close() {
                        e.snapshot.cleanup_error = Some(error);
                    }
                }
                e.backend = Some(backend);
                if e.closing_since.is_none() {
                    e.phase(Phase::Running);
                }
            }
            result => {
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => CoreError::new("spawn_panic", "创建 worker 异常退出"),
                };
                e.error(error);
                if e.snapshot.reason.is_none() {
                    e.snapshot.reason = Some(FinishReason::SpawnFailed);
                }
                e.phase(Phase::Finalizing);
                e.finished();
                return;
            }
        }
    }
    if e.backend.is_none() {
        if e.closing_since.is_some() && e.launch.is_none() {
            e.finished();
        }
        return;
    }
    let io = e.backend.as_ref().unwrap().io();
    if e.control.view_failed.load(Ordering::Acquire) {
        io.discard_output();
    }
    if let Some(error) = io.take_error() {
        e.error(error);
        e.snapshot.tail_complete = false;
        e.close(FinishReason::IoFailed);
    }
    match e.backend.as_mut().unwrap().poll_exit() {
        Ok(Some(code)) if e.snapshot.exit_code.is_none() => {
            e.snapshot.exit_code = Some(code);
            if e.closing_since.is_none() {
                e.snapshot.reason = Some(FinishReason::Exited { code });
                e.phase(Phase::Draining);
                // ConPTY may keep its output pipe open after the root process exits. Initiate
                // console shutdown now, retaining normal lossless flow until the drain deadline.
                if let Err(error) = e.backend.as_mut().unwrap().begin_close() {
                    e.error(error);
                    e.close(FinishReason::IoFailed);
                }
            }
        }
        Err(error) => {
            e.error(error);
            e.close(FinishReason::IoFailed);
        }
        _ => {}
    }
    // One block per session per turn guarantees a saturated terminal cannot monopolize IPC.
    if let Some(size) = io.next_output_size() {
        if e.flow.can_send(size)
            && !e.control.view_failed.load(Ordering::Acquire)
            && let Some(block) = io.pop_output()
        {
            let result = e.flow.submit(block).and_then(|bytes| {
                e.sink
                    .as_ref()
                    .ok_or_else(|| CoreError::new("view_missing", "终端视图已失效"))?
                    .output(bytes)
            });
            if let Err(error) = result {
                e.error(error);
                e.control.view_failed.store(true, Ordering::Release);
                e.flow.abandon();
                e.snapshot.tail_complete = false;
                e.close(FinishReason::ViewLost);
            }
        }
        let congested = !e.flow.can_send(size);
        if e.snapshot.congested != congested {
            e.snapshot.congested = congested;
            e.publish();
        }
    } else if e.snapshot.congested {
        e.snapshot.congested = false;
        e.publish();
    }
    if e.snapshot.phase == Phase::Draining
        && (io.output_done() || e.since.elapsed() >= Duration::from_secs(2))
    {
        if !io.output_done() {
            e.snapshot.tail_complete = false;
        }
        let reason = e.snapshot.reason.clone().unwrap_or(FinishReason::IoFailed);
        e.close(reason);
    }
    if let Some(since) = e.closing_since {
        if since.elapsed() >= Duration::from_millis(1500) && !e.killed {
            match e.backend.as_mut().unwrap().force_kill() {
                Ok(()) => e.killed = true,
                Err(error) => {
                    e.snapshot.cleanup_error = Some(error.clone());
                    e.error(error);
                }
            }
        }
        if since.elapsed() >= Duration::from_millis(3500)
            || (io.output_done() && e.snapshot.exit_code.is_some())
        {
            if since.elapsed() >= Duration::from_millis(3500)
                && (!io.output_done() || e.flow.pending())
            {
                e.snapshot.tail_complete = false;
            }
            io.discard_output();
            e.backend.as_ref().unwrap().cancel_io();
        }
        e.snapshot.tail_complete &= !io.truncated.load(Ordering::Acquire);
        match e.backend.as_mut().unwrap().try_reap() {
            Ok(true) if io.output_done() => {
                e.snapshot.tail_complete &= !io.truncated.load(Ordering::Acquire);
                match e.backend.as_mut().unwrap().poll_exit() {
                    Ok(code) => e.snapshot.exit_code = code.or(e.snapshot.exit_code),
                    Err(error) => e.error(error),
                }
                e.backend = None;
                e.snapshot.cleanup_error = None;
                e.finished();
            }
            Ok(true) => {}
            Ok(false) => {
                if since.elapsed() >= Duration::from_secs(5) && e.snapshot.cleanup_error.is_none() {
                    let error = CoreError::new(
                        "cleanup_timeout",
                        "清理超过 5 秒，仍在回收资源；可等待或明确强制退出应用",
                    );
                    e.snapshot.cleanup_error = Some(error.clone());
                    e.error(error);
                    e.publish();
                }
            }
            Err(error) => {
                e.snapshot.cleanup_error = Some(error.clone());
                e.error(error);
            }
        }
    }
}
