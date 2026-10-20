use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
use tracing_subscriber::fmt::MakeWriter;
const MAX_BYTES: u64 = 10 * 1024 * 1024;
struct LogFile {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
}
#[derive(Clone)]
struct Rotating(Arc<Mutex<LogFile>>);
struct Guard<'a>(MutexGuard<'a, LogFile>);
impl<'a> MakeWriter<'a> for Rotating {
    type Writer = Guard<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        Guard(self.0.lock().unwrap())
    }
}
impl Write for Guard<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.bytes + bytes.len() as u64 > MAX_BYTES {
            self.0.file.take();
            let last = self.0.path.with_extension("log.4");
            if last.exists() {
                fs::remove_file(last)?;
            }
            for index in (1..4).rev() {
                let old = self.0.path.with_extension(format!("log.{index}"));
                if old.exists() {
                    fs::rename(
                        old,
                        self.0.path.with_extension(format!("log.{}", index + 1)),
                    )?;
                }
            }
            fs::rename(&self.0.path, self.0.path.with_extension("log.1"))?;
            self.0.bytes = 0;
        }
        if self.0.file.is_none() {
            self.0.file = Some(
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&self.0.path)?,
            );
        }
        let count = self.0.file.as_mut().unwrap().write(bytes)?;
        self.0.bytes += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.file.as_mut().map_or(Ok(()), Write::flush)
    }
}
pub fn initialize(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join("terminder.log");
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let bytes = file.metadata()?.len();
    let writer = Rotating(Arc::new(Mutex::new(LogFile {
        path,
        file: Some(file),
        bytes,
    })));
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
    Ok(())
}
