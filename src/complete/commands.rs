use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

/// Builtins that are useful for completion but are not files in `PATH`.
pub const BUILTINS: &[&str] = &[
    "alias",
    "bg",
    "bind",
    "break",
    "builtin",
    "cd",
    "command",
    "continue",
    "declare",
    "dirs",
    "disown",
    "echo",
    "eval",
    "exec",
    "exit",
    "export",
    "false",
    "fc",
    "fg",
    "functions",
    "getopts",
    "hash",
    "history",
    "jobs",
    "kill",
    "let",
    "local",
    "logout",
    "popd",
    "printf",
    "pushd",
    "pwd",
    "read",
    "readonly",
    "return",
    "set",
    "shift",
    "shopt",
    "source",
    "suspend",
    "test",
    "times",
    "trap",
    "true",
    "type",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
    "wait",
    "which",
];

const MAX_COMMANDS: usize = 10_000;
const MAX_BYTES: usize = 512 * 1024;

/// Scan `PATH` and return the basenames of executable files. This runs on a
/// worker thread; it never touches the UI thread.
pub fn index_path() -> Vec<String> {
    let Some(path) = env::var_os("PATH") else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut commands = Vec::new();
    let mut bytes = 0usize;

    'dirs: for directory in env::split_paths(&path) {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() && !file_type.is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.is_empty() || name.len() > 100 || !executable(&entry.path()) {
                continue;
            }
            if seen.insert(name.clone()) {
                bytes += name.len();
                commands.push(name);
                if commands.len() >= MAX_COMMANDS || bytes >= MAX_BYTES {
                    break 'dirs;
                }
            }
        }
    }

    commands.sort_unstable();
    commands
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    let extension = format!(".{}", extension.to_ascii_lowercase());
    env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .any(|candidate| candidate.eq_ignore_ascii_case(&extension))
}

#[cfg(not(any(unix, windows)))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_sorted_and_unique() {
        let mut sorted = BUILTINS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), BUILTINS);

        let mut seen = HashSet::new();
        for builtin in BUILTINS {
            assert!(seen.insert(*builtin));
        }
    }
}
