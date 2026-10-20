//! Windows ownership boundary: every handle is non-inheritable and owned exactly once.
//! The process joins its kill-on-close Job in CreateProcess, before it can run user code.
use super::*;
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr::{null, null_mut},
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, IO::CancelSynchronousIo, JobObjects::*, Pipes::CreatePipe, Threading::*},
};

fn last(stage: &str) -> CoreError {
    CoreError::io(stage, std::io::Error::last_os_error())
}
fn owned(handle: HANDLE, stage: &str) -> Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(last(stage));
    }
    // SAFETY: the successful Windows call returned a new, uniquely owned kernel handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}
fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(CoreError::new("nul_argument", "路径或参数含 NUL"));
    }
    units.push(0);
    Ok(units)
}
fn quote(argument: &OsStr) -> Result<Vec<u16>> {
    let units = wide(argument)?;
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for &unit in &units[..units.len() - 1] {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        out.push(unit);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    Ok(out)
}
fn command_line(spec: &SpawnSpec) -> Result<Vec<u16>> {
    let mut line = quote(spec.program.as_os_str())?;
    for arg in &spec.args {
        line.push(b' ' as u16);
        line.extend(quote(arg)?);
    }
    line.push(0);
    if line.len() > 32767 {
        return Err(CoreError::new(
            "command_too_long",
            "Windows 命令行超过 32767 个 UTF-16 单元",
        ));
    }
    Ok(line)
}
fn environment(spec: &SpawnSpec) -> Result<Vec<u16>> {
    use crate::windows_env::EnvironmentKey;
    let mut vars = std::collections::BTreeMap::new();
    for (k, v) in std::env::vars_os() {
        vars.insert(EnvironmentKey::new(&k), (k, v));
    }
    for (k, v) in &spec.env {
        if let Some(v) = v {
            vars.insert(EnvironmentKey::new(k), (k.clone(), v.clone()));
        } else {
            vars.remove(&EnvironmentKey::new(k));
        }
    }
    let mut block = Vec::new();
    for (_, (k, v)) in vars {
        let mut k = wide(&k)?;
        k.pop();
        block.extend(k);
        block.push(b'=' as u16);
        block.extend(wide(&v)?);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    if block.len() > 1024 * 1024 {
        return Err(CoreError::new(
            "environment_too_large",
            "环境块超过 1 Mi UTF-16 单元",
        ));
    }
    Ok(block)
}
fn pipe() -> Result<(OwnedHandle, OwnedHandle)> {
    let mut read = null_mut();
    let mut write = null_mut();
    // SAFETY: output pointers are valid; NULL security attributes make both handles non-inheritable.
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(last("create_pipe"));
    }
    Ok((owned(read, "pipe_read")?, owned(write, "pipe_write")?))
}
struct Attributes {
    memory: Vec<usize>,
    job_list: Box<[HANDLE; 1]>,
    initialized: bool,
}
impl Attributes {
    fn new(console: HPCON, job: &OwnedHandle) -> Result<Self> {
        let mut bytes = 0;
        // SAFETY: this first call queries required storage only.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
        }
        let mut attrs = Self {
            memory: vec![0; bytes.div_ceil(size_of::<usize>())],
            job_list: Box::new([job.as_raw_handle()]),
            initialized: false,
        };
        // SAFETY: Vec<usize> is suitably aligned and lives through CreateProcess and deletion.
        if unsafe { InitializeProcThreadAttributeList(attrs.ptr(), 2, 0, &mut bytes) } == 0 {
            return Err(last("init_attributes"));
        }
        attrs.initialized = true;
        // HPCON uses the attribute value itself; JOB_LIST uses a pointer to a HANDLE array.
        if unsafe {
            UpdateProcThreadAttribute(
                attrs.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const c_void,
                size_of::<HPCON>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(last("console_attribute"));
        }
        let job_list = attrs.job_list.as_ptr();
        if unsafe {
            UpdateProcThreadAttribute(
                attrs.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                job_list.cast(),
                size_of::<HANDLE>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(last("job_attribute"));
        }
        // lpValue storage must remain live until the attribute list is destroyed.
        Ok(attrs)
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.memory.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.ptr());
            }
        }
    }
}

struct Native {
    console: Option<HPCON>,
    job: Option<OwnedHandle>,
    process: Option<OwnedHandle>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    closer: Option<JoinHandle<()>>,
    io: Arc<IoState>,
    exit_code: Option<u32>,
}
// SAFETY: console and process operations have one supervisor owner. Worker handles are disjoint.
unsafe impl Send for Native {}

pub fn spawn(spec: SpawnSpec, budget: Arc<GlobalBudget>) -> Result<Box<dyn Backend>> {
    spec.validate()?;
    let program = wide(spec.program.as_os_str())?;
    let cwd = wide(spec.cwd.as_os_str())?;
    let mut line = command_line(&spec)?;
    let env = environment(&spec)?;
    let job = owned(unsafe { CreateJobObjectW(null(), null()) }, "create_job")?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(last("job_limits"));
    }
    let (pty_input, writer) = pipe()?;
    let (reader, pty_output) = pipe()?;
    let mut console = 0;
    let hr = unsafe {
        CreatePseudoConsole(
            COORD {
                X: spec.size.cols as i16,
                Y: spec.size.rows as i16,
            },
            pty_input.as_raw_handle(),
            pty_output.as_raw_handle(),
            0,
            &mut console,
        )
    };
    if hr < 0 {
        return Err(CoreError {
            code: "conpty_create".into(),
            message: "无法创建 ConPTY，请检查 Windows 版本和系统终端组件".into(),
            stage: "conpty_create".into(),
            os_code: Some(hr),
        });
    }
    drop(pty_input);
    drop(pty_output);
    let io = Arc::new(IoState::default());
    let mut native = Native {
        console: Some(console),
        job: Some(job),
        process: None,
        reader: None,
        writer: None,
        closer: None,
        io: io.clone(),
        exit_code: None,
    };
    // Start the reader before any user process can fill the ConPTY output pipe.
    native.reader = Some(
        thread::Builder::new()
            .name("pty-read".into())
            .spawn({
                let io = io.clone();
                move || read_loop(File::from(reader), io, budget)
            })
            .map_err(|e| CoreError::io("reader_start", e))?,
    );
    native.writer = Some(
        thread::Builder::new()
            .name("pty-write".into())
            .spawn({
                let io = io.clone();
                move || write_loop(File::from(writer), io)
            })
            .map_err(|e| CoreError::io("writer_start", e))?,
    );
    let mut attrs = Attributes::new(console, native.job.as_ref().unwrap())?;
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    // Explicit NULL standard handles prevent redirected host pipes from bypassing ConPTY.
    // See microsoft/node-pty conpty.cc and microsoft/terminal discussion #15814.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.lpAttributeList = attrs.ptr();
    let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
    // SAFETY: all buffers are NUL terminated and live for this call; handles are not inherited.
    // JOB_LIST atomically establishes process ownership, including nested-Job failures.
    if unsafe {
        CreateProcessW(
            program.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            env.as_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        )
    } == 0
    {
        return Err(last("create_process"));
    }
    native.process = Some(owned(process.hProcess, "process_handle")?);
    drop(owned(process.hThread, "initial_thread")?);
    Ok(Box::new(native))
}
impl Backend for Native {
    fn io(&self) -> Arc<IoState> {
        self.io.clone()
    }
    fn resize(&mut self, size: TerminalSize) -> Result<()> {
        size.validate()?;
        let console = self
            .console
            .ok_or_else(|| CoreError::new("closing", "终端正在关闭"))?;
        let result = unsafe {
            ResizePseudoConsole(
                console,
                COORD {
                    X: size.cols as i16,
                    Y: size.rows as i16,
                },
            )
        };
        if result < 0 {
            return Err(CoreError {
                code: "resize_failed".into(),
                message: "终端尺寸更新失败".into(),
                stage: "resize".into(),
                os_code: Some(result),
            });
        }
        Ok(())
    }
    fn poll_exit(&mut self) -> Result<Option<u32>> {
        if self.exit_code.is_some() {
            return Ok(self.exit_code);
        }
        if let Some(process) = &self.process {
            match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
                        return Err(last("exit_code"));
                    }
                    self.exit_code = Some(code);
                }
                WAIT_FAILED => return Err(last("wait_process")),
                _ => {}
            }
        }
        Ok(self.exit_code)
    }
    fn begin_close(&mut self) -> Result<()> {
        if let Some(console) = self.console {
            // Pre-26100 Windows may block here. The supervisor and drain reader stay independent.
            let closer = thread::Builder::new()
                .name("pty-close".into())
                .spawn(move || unsafe {
                    ClosePseudoConsole(console);
                })
                .map_err(|e| CoreError::io("closer_start", e))?;
            self.console = None;
            self.closer = Some(closer);
        }
        Ok(())
    }
    fn force_kill(&mut self) -> Result<()> {
        if let Some(job) = &self.job
            && unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0
        {
            return Err(last("terminate_job"));
        }
        Ok(())
    }
    fn cancel_io(&self) {
        self.io.stop();
        for worker in [&self.reader, &self.writer].into_iter().flatten() {
            // ERROR_NOT_FOUND only means no call was pending. The supervisor retries to close races.
            unsafe {
                CancelSynchronousIo(worker.as_raw_handle());
            }
        }
    }
    fn try_reap(&mut self) -> Result<bool> {
        if self.console.is_some() || self.closer.as_ref().is_some_and(|h| !h.is_finished()) {
            return Ok(false);
        }
        if self.reader.as_ref().is_some_and(|h| !h.is_finished())
            || self.writer.as_ref().is_some_and(|h| !h.is_finished())
        {
            return Ok(false);
        }
        if self.process.is_some() && self.poll_exit()?.is_none() {
            return Ok(false);
        }
        // Root exit is not enough: every process associated with the Job must have exited.
        if let Some(job) = &self.job {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
            if unsafe {
                QueryInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    null_mut(),
                )
            } == 0
            {
                return Err(last("job_accounting"));
            }
            if info.ActiveProcesses != 0 {
                return Ok(false);
            }
        }
        for worker in [&mut self.reader, &mut self.writer, &mut self.closer] {
            if let Some(worker) = worker.take() {
                worker
                    .join()
                    .map_err(|_| CoreError::new("worker_panic", "I/O worker 异常退出"))?;
            }
        }
        self.process.take();
        self.job.take();
        Ok(true)
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        // This fallback is used on partial construction too; terminate before waiting for any worker.
        let _ = self.force_kill();
        self.io.drain();
        if let Some(console) = self.console.take() {
            unsafe {
                ClosePseudoConsole(console);
            }
        }
        self.cancel_io();
        while [&self.reader, &self.writer]
            .into_iter()
            .flatten()
            .any(|h| !h.is_finished())
        {
            self.cancel_io();
            std::thread::sleep(Duration::from_millis(10));
        }
        for worker in [&mut self.reader, &mut self.writer, &mut self.closer] {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quoted(s: &str) -> String {
        String::from_utf16(&quote(OsStr::new(s)).unwrap()).unwrap()
    }
    #[test]
    fn bare_conpty_creation_releases_its_handles() {
        fn cycle() {
            let (input, writer) = pipe().unwrap();
            let (reader, output) = pipe().unwrap();
            let mut console = 0;
            assert_eq!(
                unsafe {
                    CreatePseudoConsole(
                        COORD { X: 80, Y: 24 },
                        input.as_raw_handle(),
                        output.as_raw_handle(),
                        0,
                        &mut console,
                    )
                },
                0
            );
            drop(input);
            drop(output);
            let worker = std::thread::spawn(move || {
                let mut file = File::from(reader);
                let mut bytes = [0; 4096];
                while matches!(std::io::Read::read(&mut file,&mut bytes),Ok(n) if n>0) {}
            });
            unsafe {
                ClosePseudoConsole(console);
            }
            drop(writer);
            worker.join().unwrap();
        }
        cycle();
        let mut before = 0;
        unsafe {
            GetProcessHandleCount(GetCurrentProcess(), &mut before);
        }
        for _ in 0..4 {
            cycle();
        }
        let mut after = 0;
        unsafe {
            GetProcessHandleCount(GetCurrentProcess(), &mut after);
        }
        assert!(
            after <= before + 1,
            "bare ConPTY handle growth {before} -> {after}"
        );
    }
    #[test]
    fn windows_argument_quoting_preserves_empty_quotes_and_trailing_slashes() {
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("a b"), "\"a b\"");
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
        assert_eq!(quoted("C:\\a\\"), "\"C:\\a\\\\\"");
        assert_eq!(quoted("中文"), "\"中文\"");
    }
}
