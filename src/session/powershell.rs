use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tracing::{debug, warn};

use crate::error::{AppError, Result};
use crate::session::command::SessionCommand;
use crate::session::event::SessionEvent;
use crate::session::{ShellKind, TabId};

#[derive(Clone, Debug)]
pub struct ShellLaunch {
    pub shell_path: PathBuf,
    pub shell_kind: ShellKind,
    pub startup_args: Vec<String>,
}

pub fn spawn_session(
    tab_id: TabId,
    shell: ShellLaunch,
    cwd: PathBuf,
    command_rx: Receiver<SessionCommand>,
    event_tx: Sender<SessionEvent>,
) -> Result<()> {
    let pty_system = native_pty_system();
    let pty_pair = pty_system
        .openpty(PtySize {
            rows: 34,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| AppError::Session(format!("failed to create ConPTY: {err}")))?;

    let shell_path = shell.shell_path.to_string_lossy().to_string();
    let mut command = CommandBuilder::new(shell_path);
    for arg in &shell.startup_args {
        command.arg(arg);
    }
    command.cwd(cwd.clone());
    command.env("TERM", "xterm-256color");
    command.env("TERM_PROGRAM", "Terminder");
    command.env("COLORTERM", "truecolor");

    let mut child = pty_pair
        .slave
        .spawn_command(command)
        .map_err(|err| AppError::Session(format!("failed to spawn shell on ConPTY: {err}")))?;
    drop(pty_pair.slave);

    let reader = pty_pair
        .master
        .try_clone_reader()
        .map_err(|err| AppError::Session(format!("failed to clone PTY reader: {err}")))?;
    let mut writer = pty_pair
        .master
        .take_writer()
        .map_err(|err| AppError::Session(format!("failed to open PTY writer: {err}")))?;
    let master = pty_pair.master;

    let pid = child.process_id().unwrap_or_default();

    let output_tx = event_tx.clone();
    std::thread::Builder::new()
        .name(format!("terminder-tab-{}-reader", tab_id.0))
        .spawn(move || read_stream(tab_id, reader, output_tx))
        .map_err(|err| AppError::Session(format!("failed to start PTY reader: {err}")))?;

    let control_tx = event_tx.clone();
    std::thread::Builder::new()
        .name(format!("terminder-tab-{}-control", tab_id.0))
        .spawn(move || {
            let _ = control_tx.send(SessionEvent::Started { tab_id, pid });
            let _ = control_tx.send(SessionEvent::CwdChanged { tab_id, cwd });

            loop {
                match command_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(SessionCommand::Write { tab_id: cid, bytes }) if cid == tab_id => {
                        if let Err(err) = writer.write_all(&bytes) {
                            let _ = control_tx.send(SessionEvent::Error {
                                tab_id,
                                message: format!("write failed: {err}"),
                            });
                        }
                        let _ = writer.flush();
                    }
                    Ok(SessionCommand::Resize {
                        tab_id: cid,
                        cols,
                        rows,
                    }) if cid == tab_id => {
                        let resize_result = master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                        if let Err(err) = resize_result {
                            let _ = control_tx.send(SessionEvent::Error {
                                tab_id,
                                message: format!("resize failed: {err}"),
                            });
                        } else {
                            debug!("resize tab={} cols={} rows={}", tab_id.0, cols, rows);
                        }
                    }
                    Ok(SessionCommand::SetTitle { tab_id: cid, title }) if cid == tab_id => {
                        let _ = control_tx.send(SessionEvent::TitleChanged { tab_id, title });
                    }
                    Ok(SessionCommand::Close { tab_id: cid, force }) if cid == tab_id => {
                        if force {
                            let _ = child.kill();
                        } else {
                            let _ = writer.write_all(b"exit\r");
                            let _ = writer.flush();
                            graceful_exit(&mut child);
                        }
                    }
                    Ok(_) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        let _ = child.kill();
                    }
                }

                match child.try_wait() {
                    Ok(Some(status)) => {
                        let _ = control_tx.send(SessionEvent::Exited {
                            tab_id,
                            code: i32::try_from(status.exit_code()).ok(),
                        });
                        break;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        let _ = control_tx.send(SessionEvent::Error {
                            tab_id,
                            message: format!("wait failed: {err}"),
                        });
                        break;
                    }
                }
            }
        })
        .map_err(|err| AppError::Session(format!("failed to start control thread: {err}")))?;

    Ok(())
}

fn read_stream(tab_id: TabId, mut reader: Box<dyn Read + Send>, event_tx: Sender<SessionEvent>) {
    let mut buffer = vec![0_u8; 8192];

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                let payload = buffer[..size].to_vec();
                if event_tx
                    .send(SessionEvent::Output {
                        tab_id,
                        bytes: payload,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(err) => {
                let _ = event_tx.send(SessionEvent::Error {
                    tab_id,
                    message: format!("output read failed: {err}"),
                });
                break;
            }
        }
    }
}

fn graceful_exit(child: &mut Box<dyn portable_pty::Child + Send + Sync>) {
    let deadline = Instant::now() + Duration::from_millis(1_500);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                warn!("graceful exit wait failed: {err}");
                break;
            }
        }
    }
}
