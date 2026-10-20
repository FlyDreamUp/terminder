use crate::{
    model::*,
    transport::{GlobalBudget, OutputBlock},
};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(windows)]
mod windows;

pub trait Backend: Send {
    fn io(&self) -> Arc<IoState>;
    fn resize(&mut self, size: TerminalSize) -> Result<()>;
    fn poll_exit(&mut self) -> Result<Option<u32>>;
    fn begin_close(&mut self) -> Result<()>;
    fn force_kill(&mut self) -> Result<()>;
    fn cancel_io(&self);
    /// True means all workers have been joined and all process/PTY handles released.
    fn try_reap(&mut self) -> Result<bool>;
}
pub type Factory =
    Arc<dyn Fn(SpawnSpec, Arc<GlobalBudget>) -> Result<Box<dyn Backend>> + Send + Sync>;
pub fn native_factory() -> Factory {
    Arc::new(|spec, budget| {
        #[cfg(windows)]
        {
            if !cfg!(feature = "test-fixture") {
                ensure_supported_platform()?;
            }
            windows::spawn(spec, budget)
        }
        #[cfg(not(windows))]
        {
            let _ = (spec, budget);
            Err(CoreError::new(
                "unsupported_platform",
                "当前版本仅实现 Windows ConPTY，Unix 后端尚未交付",
            ))
        }
    })
}
// Ordinary builds admit the Windows 10 baseline exercised by native integration checks.
// Passing this gate does not certify resource stability on that Windows build.
pub const MIN_WINDOWS_BUILD: u32 = 19044;
pub fn windows_build() -> Option<u32> {
    #[cfg(windows)]
    {
        Some(windows_version::OsVersion::current().build)
    }
    #[cfg(not(windows))]
    {
        None
    }
}
pub fn ensure_supported_platform() -> Result<()> {
    check_windows_build(windows_build())
}
fn check_windows_build(build: Option<u32>) -> Result<()> {
    match build {
        Some(build) if build >= MIN_WINDOWS_BUILD => Ok(()),
        Some(build) => Err(CoreError::new(
            "unsupported_windows",
            format!(
                "当前 Windows build {build} 低于最低版本要求。Terminder 需要 Windows 10 21H2（build {MIN_WINDOWS_BUILD}+）或更新版本，未创建会话。"
            ),
        )),
        None => Err(CoreError::new(
            "unsupported_platform",
            "当前版本的会话后端需要 Windows 10 21H2 或更新版本",
        )),
    }
}
#[derive(Default)]
struct OutputQueue {
    blocks: VecDeque<OutputBlock>,
    bytes: usize,
    eof: bool,
    error: Option<CoreError>,
}
#[derive(Default)]
struct InputQueue {
    blocks: VecDeque<Vec<u8>>,
    bytes: usize,
    reservations: BTreeMap<String, InputReservation>,
}
struct InputReservation {
    bytes: usize,
    blocks: usize,
    expires: Instant,
}
impl InputQueue {
    fn expire(&mut self) {
        let now = Instant::now();
        self.reservations.retain(|_, r| r.expires > now);
    }
    fn can_accept(&self, bytes: usize, blocks: usize) -> bool {
        self.bytes + self.reservations.values().map(|r| r.bytes).sum::<usize>() + bytes
            <= INPUT_BUDGET
            && self.blocks.len()
                + self.reservations.values().map(|r| r.blocks).sum::<usize>()
                + blocks
                <= 128
    }
}
#[derive(Default)]
pub struct IoState {
    output: Mutex<OutputQueue>,
    input: Mutex<InputQueue>,
    wake_reader: Condvar,
    wake_writer: Condvar,
    pub stopping: AtomicBool,
    pub draining: AtomicBool,
    pub truncated: AtomicBool,
    pub write_failed: Mutex<Option<CoreError>>,
}
impl IoState {
    pub fn reserve_input(&self, bytes: usize) -> Result<String> {
        if self.draining.load(Ordering::Acquire) {
            return Err(CoreError::new("not_running", "会话已停止接收输入"));
        }
        if bytes == 0 || bytes > INPUT_BUDGET {
            return Err(CoreError::new("input_size", "输入预约长度无效"));
        }
        let mut queue = self.input.lock().unwrap();
        queue.expire();
        let blocks = bytes.div_ceil(INPUT_CHUNK);
        if !queue.can_accept(bytes, blocks) {
            return Err(CoreError::new(
                "input_full",
                "输入队列没有整次粘贴所需额度，此次粘贴未发送",
            ));
        }
        let token = uuid::Uuid::new_v4().to_string();
        queue.reservations.insert(
            token.clone(),
            InputReservation {
                bytes,
                blocks,
                expires: Instant::now() + Duration::from_secs(5),
            },
        );
        Ok(token)
    }
    pub fn enqueue_input(&self, bytes: Vec<u8>) -> Result<()> {
        self.enqueue_reserved(bytes, None)
    }
    pub fn enqueue_reserved(&self, bytes: Vec<u8>, reservation: Option<&str>) -> Result<()> {
        if self.draining.load(Ordering::Acquire) {
            return Err(CoreError::new("not_running", "会话已停止接收输入"));
        }
        if bytes.is_empty() || bytes.len() > INPUT_CHUNK {
            return Err(CoreError::new("input_size", "输入块长度无效"));
        }
        let mut queue = self.input.lock().unwrap();
        queue.expire();
        if let Some(token) = reservation {
            let reserved = queue.reservations.get_mut(token).ok_or_else(|| {
                CoreError::new(
                    "input_reservation",
                    "粘贴预约已过期或不属于此会话，剩余输入未接受",
                )
            })?;
            if bytes.len() != reserved.bytes.min(INPUT_CHUNK) {
                return Err(CoreError::new("input_reservation", "粘贴分块与预约不匹配"));
            }
            reserved.bytes -= bytes.len();
            reserved.blocks -= 1;
            if reserved.bytes == 0 {
                queue.reservations.remove(token);
            }
        } else if !queue.can_accept(bytes.len(), 1) {
            return Err(CoreError::new("input_full", "终端输入繁忙，此数据尚未接受"));
        }
        queue.bytes += bytes.len();
        queue.blocks.push_back(bytes);
        self.wake_writer.notify_one();
        Ok(())
    }
    pub fn next_output_size(&self) -> Option<usize> {
        self.output
            .lock()
            .unwrap()
            .blocks
            .front()
            .map(|b| b.bytes.len())
    }
    pub fn pop_output(&self) -> Option<OutputBlock> {
        let mut queue = self.output.lock().unwrap();
        let block = queue.blocks.pop_front()?;
        queue.bytes -= block.bytes.len();
        self.wake_reader.notify_one();
        Some(block)
    }
    pub fn output_done(&self) -> bool {
        let queue = self.output.lock().unwrap();
        queue.eof && queue.blocks.is_empty()
    }
    pub fn take_error(&self) -> Option<CoreError> {
        self.output
            .lock()
            .unwrap()
            .error
            .take()
            .or_else(|| self.write_failed.lock().unwrap().take())
    }
    pub fn wake(&self) {
        self.wake_reader.notify_one();
    }
    pub fn drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.input.lock().unwrap().reservations.clear();
        self.wake_reader.notify_all();
        self.wake_writer.notify_all();
    }
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.drain();
    }
    pub fn discard_output(&self) {
        let mut q = self.output.lock().unwrap();
        if !q.blocks.is_empty() {
            self.truncated.store(true, Ordering::Release);
        }
        q.blocks.clear();
        q.bytes = 0;
        self.wake_reader.notify_one();
    }
    pub fn input_bytes(&self) -> usize {
        self.input.lock().unwrap().bytes
    }
}

pub fn read_loop(mut reader: impl Read, io: Arc<IoState>, budget: Arc<GlobalBudget>) {
    loop {
        if io.stopping.load(Ordering::Acquire) {
            break;
        }
        // Keep a live drain reader during ClosePseudoConsole even if the view cannot ACK.
        let drain = io.draining.load(Ordering::Acquire);
        let lease = {
            let queue = io.output.lock().unwrap();
            if !drain
                && (queue.bytes + OUTPUT_CHUNK > OUTPUT_QUEUE_BYTES
                    || queue.blocks.len() >= OUTPUT_QUEUE_BLOCKS)
            {
                let _ = io
                    .wake_reader
                    .wait_timeout(queue, Duration::from_millis(25))
                    .unwrap();
                continue;
            }
            drop(queue);
            budget.reserve(OUTPUT_CHUNK)
        };
        if lease.is_none() && !drain {
            let q = io.output.lock().unwrap();
            let _ = io
                .wake_reader
                .wait_timeout(q, Duration::from_millis(25))
                .unwrap();
            continue;
        }
        let mut bytes = vec![0; OUTPUT_CHUNK];
        match reader.read(&mut bytes) {
            Ok(0) => break,
            Ok(count) => {
                bytes.truncate(count);
                let mut q = io.output.lock().unwrap();
                if let Some(mut lease) = lease.filter(|_| {
                    q.bytes + count <= OUTPUT_QUEUE_BYTES && q.blocks.len() < OUTPUT_QUEUE_BLOCKS
                }) {
                    lease.shrink(count);
                    q.bytes += count;
                    q.blocks.push_back(OutputBlock { bytes, lease });
                } else {
                    io.truncated.store(true, Ordering::Release);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                ) || io.stopping.load(Ordering::Acquire) =>
            {
                break;
            }
            Err(e) => {
                io.output.lock().unwrap().error = Some(CoreError::io("pty_read", e));
                break;
            }
        }
    }
    io.output.lock().unwrap().eof = true;
}

pub fn write_loop(mut writer: impl Write, io: Arc<IoState>) {
    loop {
        let bytes = {
            let mut q = io.input.lock().unwrap();
            while q.blocks.is_empty() && !io.draining.load(Ordering::Acquire) {
                q = io.wake_writer.wait(q).unwrap();
            }
            if io.draining.load(Ordering::Acquire) {
                if q.bytes != 0 {
                    *io.write_failed.lock().unwrap() = Some(CoreError::new(
                        "input_incomplete",
                        "已接受的输入未全部写入，未自动重试",
                    ));
                }
                q.blocks.clear();
                q.bytes = 0;
                break;
            }
            q.blocks.pop_front().expect("queue nonempty")
        };
        let result = writer.write_all(&bytes);
        io.input.lock().unwrap().bytes -= bytes.len();
        if let Err(error) = result {
            if !io.stopping.load(Ordering::Acquire) {
                *io.write_failed.lock().unwrap() = Some(CoreError::io("pty_write", error));
            }
            break;
        }
    }
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    #[test]
    fn windows_10_21h2_and_newer_pass_the_platform_gate() {
        for build in [19044, 19045, 22000, 26100] {
            assert!(check_windows_build(Some(build)).is_ok(), "build {build}");
        }
    }

    #[test]
    fn older_windows_builds_are_rejected_with_the_required_version() {
        for build in [0, 17763, 19043] {
            let error = check_windows_build(Some(build)).unwrap_err();
            assert_eq!(error.code, "unsupported_windows");
            assert_eq!(error.stage, "unsupported_windows");
            assert!(error.message.contains(&format!("build {build}")));
            assert!(error.message.contains("Windows 10 21H2"));
            assert!(error.message.contains("19044+"));
        }
    }

    #[test]
    fn non_windows_platforms_remain_unsupported() {
        let error = check_windows_build(None).unwrap_err();
        assert_eq!(error.code, "unsupported_platform");
        assert_eq!(error.stage, "unsupported_platform");
    }
}
