use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::shell::{CwdMode, TerminalProfile, wsl_directory};
use crate::workspace::PaneId;

/// A PTY and the child process attached to it.
pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: PtySize,
}

impl Session {
    pub fn spawn(
        pane: PaneId,
        rows: usize,
        cols: usize,
        proxy: EventLoopProxy<UserEvent>,
        profile: &TerminalProfile,
        cwd: &Path,
    ) -> Result<Self> {
        let pty_system = native_pty_system();
        let size = PtySize {
            rows: rows.max(1) as u16,
            cols: cols.max(2) as u16,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).context("failed to open a PTY")?;

        let mut command = CommandBuilder::new(&profile.program);
        for arg in &profile.args {
            command.arg(arg);
        }
        command.env("F_TERMINAL_PANE", pane.0.to_string());

        match profile.cwd_mode {
            CwdMode::Native => {
                command.cwd(cwd);
            }
            CwdMode::Wsl { .. } => {
                if let Some(directory) = wsl_directory(cwd) {
                    command.arg("--cd");
                    command.arg(directory);
                }
                if cwd.is_dir() {
                    command.cwd(cwd);
                }
            }
        }

        // Make sure the child does not inherit a TERM value from whatever
        // terminal launched us, and advertise 24-bit color.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "f-terminal");

        for (key, value) in &profile.env {
            command.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to spawn the shell")?;

        // The slave side must be closed in the parent or reads would never see
        // EOF when the child exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone the PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take the PTY writer")?;

        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buffer = vec![0u8; 256 * 1024];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            if proxy
                                .send_event(UserEvent::PtyData {
                                    pane,
                                    bytes: buffer[..read].to_vec(),
                                })
                                .is_err()
                            {
                                // UI is gone.
                                return;
                            }
                        }
                        Err(error) => {
                            // EIO is the normal way a PTY reports child exit.
                            log::debug!("PTY read ended: {error}");
                            break;
                        }
                    }
                }
                let _ = proxy.send_event(UserEvent::PtyClosed { pane });
            })
            .context("failed to spawn the PTY reader thread")?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            size,
        })
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if let Err(error) = self.writer.write_all(bytes) {
            log::debug!("PTY write failed: {error}");
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let size = PtySize {
            rows: rows.max(1) as u16,
            cols: cols.max(2) as u16,
            pixel_width: 0,
            pixel_height: 0,
        };
        if size.rows == self.size.rows && size.cols == self.size.cols {
            return;
        }
        self.size = size;
        if let Err(error) = self.master.resize(size) {
            log::warn!("PTY resize failed: {error}");
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Best effort: make sure the shell does not survive the window.
        let _ = self.child.kill();
    }
}
