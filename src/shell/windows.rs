use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{CwdMode, TerminalProfile};

pub fn detect(profiles: &mut Vec<TerminalProfile>, preferred: Option<&str>) {
    let mut candidates = Vec::new();

    if let Some(preferred) = preferred
        && (preferred.contains('\\') || preferred.contains('/'))
    {
        let path = PathBuf::from(preferred);
        if path.is_file() {
            candidates.push(custom_profile(path));
        }
    }

    if let Some(path) = find_pwsh() {
        candidates.push(simple(
            "windows:pwsh",
            "PowerShell 7",
            "pwsh",
            path,
            vec!["-NoLogo"],
        ));
    }

    if let Some(path) = windows_powershell() {
        candidates.push(simple(
            "windows:powershell",
            "Windows PowerShell",
            "PS",
            path,
            vec!["-NoLogo"],
        ));
    }

    if let Some(path) = cmd_exe() {
        candidates.push(simple(
            "windows:cmd",
            "Command Prompt",
            "cmd",
            path,
            Vec::new(),
        ));
    }

    if let Some(path) = git_bash() {
        candidates.push(simple(
            "windows:git-bash",
            "Git Bash",
            "bash",
            path,
            vec!["--login", "-i"],
        ));
    }

    if let Some(path) = msys2_bash() {
        candidates.push(simple(
            "windows:msys2",
            "MSYS2 Bash",
            "bash",
            path,
            vec!["--login", "-i"],
        ));
    }

    if let Some(path) = cygwin_bash() {
        candidates.push(simple(
            "windows:cygwin",
            "Cygwin Bash",
            "bash",
            path,
            vec!["--login", "-i"],
        ));
    }

    if let Some(wsl) = wsl_exe() {
        let env = wsl_env();
        candidates.push(TerminalProfile {
            id: "windows:wsl".to_owned(),
            name: "WSL".to_owned(),
            short: "WSL".to_owned(),
            program: wsl.clone(),
            args: Vec::new(),
            env: env.clone(),
            cwd_mode: CwdMode::Wsl { distro: None },
        });

        for distro in wsl_distros(&wsl) {
            candidates.push(TerminalProfile {
                id: format!("windows:wsl:{distro}"),
                name: format!("{distro} (WSL)"),
                short: "WSL".to_owned(),
                program: wsl.clone(),
                args: vec!["-d".to_owned(), distro.clone()],
                env: env.clone(),
                cwd_mode: CwdMode::Wsl {
                    distro: Some(distro),
                },
            });
        }
    }

    profiles.extend(candidates);
}

fn simple(id: &str, name: &str, short: &str, program: PathBuf, args: Vec<&str>) -> TerminalProfile {
    TerminalProfile {
        id: id.to_owned(),
        name: name.to_owned(),
        short: short.to_owned(),
        program,
        args: args.into_iter().map(str::to_owned).collect(),
        env: Vec::new(),
        cwd_mode: CwdMode::Native,
    }
}

fn custom_profile(program: PathBuf) -> TerminalProfile {
    let name = program
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Shell")
        .to_owned();
    TerminalProfile {
        id: format!("windows:{}", program.display()),
        name: name.clone(),
        short: name.chars().take(4).collect(),
        program,
        args: Vec::new(),
        env: Vec::new(),
        cwd_mode: CwdMode::Native,
    }
}

fn cmd_exe() -> Option<PathBuf> {
    env::var_os("COMSPEC")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            env::var_os("SystemRoot")
                .map(|root| PathBuf::from(root).join(r"System32\cmd.exe"))
                .filter(|path| path.is_file())
        })
        .or_else(|| where_program("cmd.exe"))
}

fn windows_powershell() -> Option<PathBuf> {
    env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .filter(|path| path.is_file())
        .or_else(|| where_program("powershell.exe"))
}

fn find_pwsh() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = env::var_os(key) {
            candidates.push(PathBuf::from(&root).join(r"PowerShell\7\pwsh.exe"));
            candidates.push(PathBuf::from(&root).join(r"PowerShell\7-preview\pwsh.exe"));
        }
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Microsoft\WindowsApps\pwsh.exe"));
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| where_program("pwsh.exe"))
}

fn git_bash() -> Option<PathBuf> {
    let mut roots = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = env::var_os(key) {
            roots.push(PathBuf::from(root).join("Git"));
        }
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join(r"Programs\Git"));
    }

    if let Some(git) = where_program("git.exe")
        && let Some(root) = git.parent().and_then(Path::parent)
    {
        roots.push(root.to_path_buf());
    }

    for root in roots {
        for relative in [r"bin\bash.exe", r"usr\bin\bash.exe"] {
            let path = root.join(relative);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    None
}

fn msys2_bash() -> Option<PathBuf> {
    [r"C:\msys64\usr\bin\bash.exe", r"C:\msys32\usr\bin\bash.exe"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn cygwin_bash() -> Option<PathBuf> {
    [r"C:\cygwin64\bin\bash.exe", r"C:\cygwin\bin\bash.exe"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn wsl_exe() -> Option<PathBuf> {
    env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join(r"System32\wsl.exe"))
        .filter(|path| path.is_file())
        .or_else(|| where_program("wsl.exe"))
}

fn wsl_distros(wsl: &Path) -> Vec<String> {
    let Ok(output) = Command::new(wsl).args(["-l", "-q"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let text = decode_wsl_output(&output.stdout);
    let mut distros: Vec<String> = text
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}').trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    distros.dedup();
    distros
}

fn decode_wsl_output(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.starts_with(&[0xff, 0xfe])
        || bytes
            .iter()
            .skip(1)
            .step_by(2)
            .take(128)
            .any(|byte| *byte == 0);

    if looks_utf16 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[low, high]| u16::from_le_bytes([*low, *high]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn wsl_env() -> Vec<(String, String)> {
    vec![(
        "WSLENV".to_owned(),
        "TERM/u:COLORTERM/u:TERM_PROGRAM/u".to_owned(),
    )]
}

fn where_program(name: &str) -> Option<PathBuf> {
    let output = Command::new("where.exe").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.is_file())
}
