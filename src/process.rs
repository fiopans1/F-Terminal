use std::ffi::OsStr;
use std::process::Command;

/// Build a command for a console program.
///
/// F-Terminal is a GUI-subsystem binary on Windows. Every console child it
/// starts (git, gh, glab, curl, wsl, ...) would otherwise get its own console
/// window, which Windows 11 hosts in Windows Terminal. `CREATE_NO_WINDOW`
/// gives the child a console without a window and children inherit it, so the
/// app can keep spawning helpers in the background without flashing terminals.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}
