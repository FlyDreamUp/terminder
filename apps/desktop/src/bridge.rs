use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use terminder_session_core::{
    model::*,
    supervisor::{Sink, Supervisor},
};

pub const OWNER: &str = "native-desktop";
const INPUT_QUEUE_BYTES: usize = 1024 * 1024;
const INPUT_QUEUE_ITEMS: usize = 128;
pub enum Message {
    Output(Vec<u8>),
    Event(SessionEvent),
    Notice(String),
    Error(String),
}
#[derive(Default)]
struct Pending {
    input: VecDeque<Vec<u8>>,
    bytes: usize,
    ack: Option<(u64, u64)>,
    size: Option<TerminalSize>,
}
#[derive(Default)]
struct Mailbox {
    pending: Mutex<Pending>,
    wake: Condvar,
    closing: AtomicBool,
    finished: AtomicBool,
}
impl Mailbox {
    fn close(&self) {
        self.closing.store(true, Ordering::Release);
        self.wake.notify_one();
    }
    fn input(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        anyhow::ensure!(
            !self.closing.load(Ordering::Acquire) && !self.finished.load(Ordering::Acquire),
            "会话已关闭"
        );
        // Include the bracketed-paste envelope, and account for bytes currently being sent.
        anyhow::ensure!(bytes.len() <= 256 * 1024 + 12, "输入超过 256 KiB");
        let mut queue = self.pending.lock().unwrap();
        anyhow::ensure!(
            queue.bytes + bytes.len() <= INPUT_QUEUE_BYTES && queue.input.len() < INPUT_QUEUE_ITEMS,
            "输入队列已满，本次输入未接受"
        );
        queue.bytes += bytes.len();
        queue.input.push_back(bytes);
        self.wake.notify_one();
        Ok(())
    }
}
struct UiSink {
    sender: async_channel::Sender<Message>,
    mailbox: Arc<Mailbox>,
}
impl Sink for UiSink {
    fn output(&self, bytes: Vec<u8>) -> Result<()> {
        self.sender
            .try_send(Message::Output(bytes))
            .map_err(|_| CoreError::new("view_queue", "终端输出队列已关闭或已满"))
    }
    fn event(&self, event: SessionEvent) -> Result<()> {
        if matches!(&event, SessionEvent::State { snapshot } if snapshot.phase == Phase::Finished) {
            self.mailbox.finished.store(true, Ordering::Release);
            self.mailbox.wake.notify_one();
        }
        self.sender
            .try_send(Message::Event(event))
            .map_err(|_| CoreError::new("view_queue", "终端事件队列已关闭或已满"))
    }
}
struct Worker {
    mailbox: Arc<Mailbox>,
    thread: JoinHandle<()>,
}
struct RuntimeInner {
    supervisor: Supervisor,
    workers: Mutex<Vec<Worker>>,
    closing: AtomicBool,
}
/// The application owns this runtime until every adapter and the supervisor have joined.
#[derive(Clone)]
pub struct Runtime(Arc<RuntimeInner>);
impl Runtime {
    pub fn new(supervisor: Supervisor) -> Self {
        Self(Arc::new(RuntimeInner {
            supervisor,
            workers: Mutex::new(Vec::new()),
            closing: AtomicBool::new(false),
        }))
    }
    #[cfg(all(windows, feature = "test-support"))]
    pub fn supervisor(&self) -> &Supervisor {
        &self.0.supervisor
    }
    fn reap(&self) -> anyhow::Result<()> {
        let mut workers = self.0.workers.lock().unwrap();
        let mut index = 0;
        while index < workers.len() {
            if workers[index].thread.is_finished() {
                workers
                    .swap_remove(index)
                    .thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("会话适配线程异常退出"))?;
            } else {
                index += 1;
            }
        }
        Ok(())
    }
    pub fn begin_shutdown(&self) {
        self.0.closing.store(true, Ordering::Release);
        for worker in self.0.workers.lock().unwrap().iter() {
            worker.mailbox.close();
        }
    }
    /// Non-blocking: only join handles whose completion was already observed.
    pub fn poll_shutdown(&self) -> anyhow::Result<bool> {
        self.reap()?;
        if !self.0.workers.lock().unwrap().is_empty() {
            return Ok(false);
        }
        self.0.supervisor.shutdown();
        Ok(self.0.supervisor.join_if_stopped()?)
    }
    pub fn shutdown_blocking(&self) -> anyhow::Result<()> {
        self.begin_shutdown();
        while !self.poll_shutdown()? {
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        // Fallback for startup errors. Normal window closure calls poll_shutdown first.
        for worker in self.workers.get_mut().unwrap().iter() {
            worker.mailbox.close();
        }
        self.supervisor.shutdown();
        for worker in self.workers.get_mut().unwrap().drain(..) {
            if worker.thread.join().is_err() {
                tracing::error!("session adapter panicked");
            }
        }
    }
}
pub struct Bridge {
    mailbox: Arc<Mailbox>,
    receiver: async_channel::Receiver<Message>,
    _runtime: Runtime,
}
impl Bridge {
    pub fn new(runtime: Runtime, spec: SpawnSpec) -> anyhow::Result<Self> {
        runtime.reap()?;
        anyhow::ensure!(!runtime.0.closing.load(Ordering::Acquire), "应用正在关闭");
        let mut workers = runtime.0.workers.lock().unwrap();
        anyhow::ensure!(
            workers.len() < MAX_SESSIONS,
            "会话正在回收或数量已达上限，请稍后重试"
        );
        let (sender, receiver) = async_channel::bounded(128);
        let mailbox = Arc::new(Mailbox::default());
        let worker_mailbox = mailbox.clone();
        let supervisor = runtime.0.supervisor.clone();
        let worker = thread::Builder::new()
            .name("terminal-adapter".into())
            .spawn(move || {
                if let Err(error) = run(supervisor, spec, sender.clone(), worker_mailbox) {
                    let _ = sender.try_send(Message::Error(error.to_string()));
                }
            })?;
        workers.push(Worker {
            mailbox: mailbox.clone(),
            thread: worker,
        });
        drop(workers);
        Ok(Self {
            mailbox,
            receiver,
            _runtime: runtime,
        })
    }
    pub fn receiver(&self) -> async_channel::Receiver<Message> {
        self.receiver.clone()
    }
    pub fn input(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.mailbox.input(bytes)
    }
    pub fn ack(&self, sequence: u64, bytes: u64) -> anyhow::Result<()> {
        if !self.mailbox.finished.load(Ordering::Acquire) {
            let mut pending = self.mailbox.pending.lock().unwrap();
            anyhow::ensure!(
                pending
                    .ack
                    .is_none_or(|(seq, count)| sequence >= seq && bytes >= count),
                "消费确认顺序无效"
            );
            pending.ack = Some((sequence, bytes));
            self.mailbox.wake.notify_one();
        }
        Ok(())
    }
    pub fn resize(&self, size: TerminalSize) -> anyhow::Result<()> {
        size.validate()?;
        self.mailbox.pending.lock().unwrap().size = Some(size);
        self.mailbox.wake.notify_one();
        Ok(())
    }
    pub fn close(&self) {
        self.mailbox.close();
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.close();
        self.receiver.close();
        // Never join here: dropping a pane is an operation on the UI thread.
    }
}
fn run(
    supervisor: Supervisor,
    spec: SpawnSpec,
    sender: async_channel::Sender<Message>,
    mailbox: Arc<Mailbox>,
) -> anyhow::Result<()> {
    let size = spec.size;
    let handle = supervisor.prepare(OWNER, spec)?;
    let sink = Arc::new(UiSink {
        sender: sender.clone(),
        mailbox: mailbox.clone(),
    });
    let result = (|| -> anyhow::Result<()> {
        supervisor.start(OWNER, handle.binding.clone(), size, sink)?;
        let (mut sequence, mut revision) = (0, 0);
        while !mailbox.closing.load(Ordering::Acquire) && !mailbox.finished.load(Ordering::Acquire)
        {
            let (ack, size, input) = {
                let mut pending = mailbox.pending.lock().unwrap();
                if pending.input.is_empty() && pending.ack.is_none() && pending.size.is_none() {
                    pending = mailbox
                        .wake
                        .wait_timeout(pending, Duration::from_millis(20))
                        .unwrap()
                        .0;
                }
                (
                    pending.ack.take(),
                    pending.size.take(),
                    pending.input.pop_front(),
                )
            };
            if mailbox.finished.load(Ordering::Acquire) || mailbox.closing.load(Ordering::Acquire) {
                break;
            }
            if let Some((sequence, bytes)) = ack
                && let Err(error) = supervisor.ack(OWNER, handle.binding.clone(), sequence, bytes)
                && !mailbox.finished.load(Ordering::Acquire)
            {
                return Err(error.into());
            }
            if let Some(size) = size {
                revision += 1;
                if let Err(error) = supervisor.resize(OWNER, handle.binding.clone(), revision, size)
                    && !mailbox.finished.load(Ordering::Acquire)
                {
                    return Err(error.into());
                }
            }
            if let Some(bytes) = input {
                let length = bytes.len();
                // Reserve the complete paste before sending any of it; never retry an unknown result.
                let reservation = supervisor.reserve_input(OWNER, handle.binding.clone(), length);
                let sent = match reservation {
                    Ok(token) => {
                        let mut result = Ok(());
                        for chunk in bytes.chunks(INPUT_CHUNK) {
                            if mailbox.closing.load(Ordering::Acquire)
                                || mailbox.finished.load(Ordering::Acquire)
                            {
                                break;
                            }
                            sequence += 1;
                            result = supervisor.input_reserved(
                                OWNER,
                                handle.binding.clone(),
                                sequence,
                                chunk.to_vec(),
                                Some(token.clone()),
                            );
                            if result.is_err() {
                                break;
                            }
                        }
                        result
                    }
                    Err(error) if error.code == "input_full" => {
                        let _ = sender.try_send(Message::Notice(error.to_string()));
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                mailbox.pending.lock().unwrap().bytes -= length;
                sent?;
            }
        }
        Ok(())
    })();
    handle.close();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !mailbox.finished.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if mailbox.finished.load(Ordering::Acquire) {
        if let Err(error) = supervisor.forget(OWNER, handle.binding)
            && error.code != "shutting_down"
        {
            return Err(error.into());
        }
    } else {
        handle.view_failed();
        anyhow::bail!("会话回收超过 10 秒，内核继续清理");
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_budget_counts_bytes_and_rejects_without_partial_enqueue() {
        let mailbox = Mailbox::default();
        for _ in 0..4 {
            mailbox.input(vec![b'a'; 256 * 1024]).unwrap();
        }
        assert!(mailbox.input(vec![b'b']).is_err());
        assert_eq!(mailbox.pending.lock().unwrap().input.len(), 4);
        assert_eq!(mailbox.pending.lock().unwrap().bytes, INPUT_QUEUE_BYTES);
        mailbox.close();
        assert!(mailbox.input(vec![b'a']).is_err());
    }
}
