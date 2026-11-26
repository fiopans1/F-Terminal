use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::shell::{CwdMode, TerminalProfile, wsl_directory};
use crate::workspace::PaneId;

/// PowerShell does not keep its process current directory in sync with
/// `Set-Location`, so `sysinfo` cannot follow `cd`. Wrap the prompt so every
/// prompt reports `$PWD` through the ConEmu/Windows Terminal `OSC 9;9`.
#[cfg(windows)]
fn profile_is(profile: &TerminalProfile, id: &str, programs: &[&str]) -> bool {
    profile.id == id
        || profile
            .program
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                programs
                    .iter()
                    .any(|candidate| name.eq_ignore_ascii_case(candidate))
            })
}

#[cfg(windows)]
fn configure_powershell_cwd(command: &mut CommandBuilder, profile: &TerminalProfile) {
    let powershell = ["windows:pwsh", "windows:powershell"].iter().any(|id| {
        profile_is(
            profile,
            id,
            &["pwsh.exe", "powershell.exe", "pwsh", "powershell"],
        )
    });
    if !powershell
        || profile.args.iter().any(|arg| {
            matches!(
                arg.to_ascii_lowercase().as_str(),
                "-command" | "-c" | "-file" | "-f"
            )
        })
    {
        return;
    }

    command.arg("-NoExit");
    command.arg("-Command");
    command.arg(
        r#"$global:__ft_prompt = $function:prompt; function global:prompt { $__ft_loc = Get-Location; if ($__ft_loc.Provider.Name -eq 'FileSystem') { $__ft_esc = [char]27; [Console]::Write($__ft_esc + ']9;9;' + $__ft_loc.Path + $__ft_esc + '\') }; if ($global:__ft_prompt) { & $global:__ft_prompt } else { "PS $($__ft_loc.Path)> " } }"#,
    );
}

#[cfg(windows)]
fn configure_cmd_cwd(command: &mut CommandBuilder, profile: &TerminalProfile) {
    if !profile_is(profile, "windows:cmd", &["cmd.exe"]) {
        return;
    }
    let existing = std::env::var("PROMPT").unwrap_or_else(|_| "$P$G".to_owned());
    command.env("PROMPT", format!("$E]9;9;$P$E\\{existing}"));
}

#[cfg(windows)]
fn configure_bash_cwd(command: &mut CommandBuilder, profile: &TerminalProfile) {
    let known = ["windows:git-bash", "windows:msys2", "windows:cygwin"]
        .iter()
        .any(|id| profile_is(profile, id, &["bash.exe"]));
    if !known {
        return;
    }
    let hook = r#"printf '\033]9;9;%s\033\\' "$PWD""#;
    let existing = std::env::var("PROMPT_COMMAND").unwrap_or_default();
    let value = if existing.trim().is_empty() {
        hook.to_owned()
    } else {
        format!("{hook};{existing}")
    };
    command.env("PROMPT_COMMAND", value);
}

/// A PTY and the child process attached to it.
pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Kept unread until the pane is inserted into the workspace. This lets a
    /// pane be built on a worker thread without losing early shell output.
    reader: Option<Box<dyn Read + Send>>,
    proxy: EventLoopProxy<UserEvent>,
    pane: PaneId,
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
        #[cfg(windows)]
        {
            configure_powershell_cwd(&mut command, profile);
            configure_cmd_cwd(&mut command, profile);
            configure_bash_cwd(&mut command, profile);
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

        let spawn_started = Instant::now();
        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to spawn the shell")?;
        log::debug!(
            "spawned {} for pane {} in {:?}",
            profile.name,
            pane.0,
            spawn_started.elapsed()
        );

        // The slave side must be closed in the parent or reads would never see
        // EOF when the child exits.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone the PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take the PTY writer")?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            reader: Some(reader),
            proxy,
            pane,
            size,
        })
    }

    /// Start forwarding PTY output to the UI. Panes built on a worker thread
    /// call this only after they have been inserted into the workspace, so no
    /// output is dropped or attributed to an unknown pane.
    pub fn start_reader(&mut self) {
        let Some(mut reader) = self.reader.take() else {
            return;
        };
        let proxy = self.proxy.clone();
        let pane = self.pane;
        let spawned = std::thread::Builder::new()
            .name(format!("pty-reader-{}", pane.0))
            .spawn(move || {
                // 64 KiB is enough to drain a busy PTY without reserving a
                // large buffer for every open pane.
                let mut buffer = vec![0u8; 64 * 1024];
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
            });
        if let Err(error) = spawned {
            log::warn!(
                "failed to spawn the PTY reader for pane {}: {error}",
                pane.0
            );
        }
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
