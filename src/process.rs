use std::ffi::{OsStr, OsString};
use std::process::Command;

#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::sync::OnceLock;

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

/// Git command with a Windows fallback for installs that are not on the
/// `PATH` visible to a GUI process.
pub fn git_command() -> Command {
    command(git_program())
}

fn git_program() -> OsString {
    #[cfg(windows)]
    {
        static GIT: OnceLock<Option<PathBuf>> = OnceLock::new();
        let resolved = GIT.get_or_init(|| {
            let mut candidates = Vec::new();
            for key in ["ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(root) = std::env::var_os(key) {
                    let root = PathBuf::from(root);
                    candidates.push(root.join(r"Git\cmd\git.exe"));
                    candidates.push(root.join(r"Git\bin\git.exe"));
                }
            }
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                candidates.push(PathBuf::from(local).join(r"Programs\Git\cmd\git.exe"));
            }
            if let Some(profile) = std::env::var_os("USERPROFILE") {
                candidates.push(PathBuf::from(profile).join(r"scoop\apps\git\current\cmd\git.exe"));
            }
            if let Some(program_data) = std::env::var_os("ProgramData") {
                candidates.push(PathBuf::from(program_data).join(r"chocolatey\bin\git.exe"));
            }
            candidates.into_iter().find(|path| path.is_file())
        });
        if let Some(path) = resolved {
            return path.as_os_str().to_owned();
        }
    }

    OsString::from("git")
}
