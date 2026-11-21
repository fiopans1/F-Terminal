use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{CwdMode, TerminalProfile};

pub fn detect(profiles: &mut Vec<TerminalProfile>, preferred: Option<&str>) {
    let mut paths = Vec::new();

    if let Some(preferred) = preferred
        && let Some(path) = resolve_executable(preferred)
    {
        paths.push(path);
    }

    if let Ok(shell) = std::env::var("SHELL")
        && let Some(path) = resolve_executable(&shell)
    {
        paths.push(path);
    }

    if let Ok(contents) = fs::read_to_string("/etc/shells") {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(path) = resolve_executable(line) {
                paths.push(path);
            }
        }
    }

    for fallback in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        let path = PathBuf::from(fallback);
        if executable(&path) {
            paths.push(path);
        }
    }

    let mut seen = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().into_owned();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        profiles.push(profile_for(path));
    }
}

fn profile_for(path: PathBuf) -> TerminalProfile {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("shell")
        .to_owned();
    let login = matches!(
        name.as_str(),
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "mksh" | "tcsh"
    );

    TerminalProfile {
        id: format!("unix:{}", path.display()),
        name: name.clone(),
        short: name,
        program: path,
        args: if login {
            vec!["-l".to_owned()]
        } else {
            Vec::new()
        },
        env: Vec::new(),
        cwd_mode: CwdMode::Native,
    }
}

fn resolve_executable(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let path = Path::new(value);
    if path.is_absolute() || value.contains('/') {
        return executable(path).then(|| path.to_path_buf());
    }

    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(value))
        .find(|candidate| executable(candidate))
}

fn executable(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}
