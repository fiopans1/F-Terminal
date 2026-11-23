use std::fs;
use std::path::{Path, PathBuf};

use crate::process::command;

/// Branch and working-tree stats for a terminal pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    pub additions: usize,
    pub deletions: usize,
}

/// Inspect the repository that contains `start`.
///
/// Returns `None` when the directory is not inside a Git repository or when
/// Git is not available. The repository root and branch are resolved directly
/// from `.git`, so a poll only spawns one process (the diff), or none at all
/// outside a repository.
pub fn inspect(start: &Path) -> Option<GitStatus> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };

    let root = repo_root(start)?;
    let branch = branch_name(&root).unwrap_or_else(|| "HEAD".to_owned());

    let numstat = run_git(&root, &["diff", "--numstat", "HEAD"])
        .or_else(|| run_git(&root, &["diff", "--numstat"]))
        .unwrap_or_default();
    let (additions, deletions) = sum_numstat(&numstat);

    Some(GitStatus {
        branch,
        additions,
        deletions,
    })
}

fn repo_root(start: &Path) -> Option<PathBuf> {
    let mut directory = start;
    loop {
        if directory.join(".git").exists() || is_bare_repository(directory) {
            return Some(directory.to_path_buf());
        }
        directory = directory.parent()?;
    }
}

fn is_bare_repository(directory: &Path) -> bool {
    directory.join("HEAD").is_file()
        && directory.join("config").is_file()
        && directory.join("objects").is_dir()
        && directory.join("refs").is_dir()
}

fn branch_name(root: &Path) -> Option<String> {
    let git_directory = git_directory(root)?;
    let head = fs::read_to_string(git_directory.join("HEAD")).ok()?;
    let head = head.trim();

    if let Some(reference) = head.strip_prefix("ref: ") {
        let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        return (!branch.is_empty()).then(|| branch.to_owned());
    }

    let short: String = head.chars().take(7).collect();
    (!short.is_empty()).then_some(short)
}

fn git_directory(root: &Path) -> Option<PathBuf> {
    let dotgit = root.join(".git");

    if dotgit.is_dir() {
        return Some(dotgit);
    }

    if dotgit.is_file() {
        let text = fs::read_to_string(&dotgit).ok()?;
        let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
        let path = PathBuf::from(target);
        return Some(if path.is_absolute() {
            path
        } else {
            root.join(path)
        });
    }

    is_bare_repository(root).then(|| root.to_path_buf())
}

fn sum_numstat(text: &str) -> (usize, usize) {
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for line in text.lines() {
        let mut fields = line.split('\t');
        let Some(add) = fields.next() else {
            continue;
        };
        let Some(del) = fields.next() else {
            continue;
        };
        additions += add.parse::<usize>().unwrap_or(0);
        deletions += del.parse::<usize>().unwrap_or(0);
    }

    (additions, deletions)
}

fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "f-terminal-git-status-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sums_numstat_and_ignores_binary_files() {
        let text = "10\t2\tsrc/a.rs\n3\t0\tsrc/b.rs\n-\t-\timage.png\n";
        assert_eq!(sum_numstat(text), (13, 2));
    }

    #[test]
    fn empty_numstat_is_zero() {
        assert_eq!(sum_numstat(""), (0, 0));
    }

    #[test]
    fn finds_repository_root_and_branch_from_disk() {
        let root = scratch_dir();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(
            root.join(".git").join("HEAD"),
            "ref: refs/heads/feature/x\n",
        )
        .unwrap();

        let nested = root.join("src").join("nested");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(repo_root(&nested), Some(root.clone()));
        assert_eq!(branch_name(&root).as_deref(), Some("feature/x"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_linked_worktree_git_directory() {
        let root = scratch_dir();
        let linked_git = root.join("worktree-git-data");
        fs::create_dir_all(&linked_git).unwrap();
        fs::write(linked_git.join("HEAD"), "ref: refs/heads/work\n").unwrap();
        fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", linked_git.display()),
        )
        .unwrap();

        assert_eq!(git_directory(&root), Some(linked_git.clone()));
        assert_eq!(branch_name(&root).as_deref(), Some("work"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detached_head_uses_short_hash() {
        let root = scratch_dir();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(
            root.join(".git").join("HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();

        assert_eq!(branch_name(&root).as_deref(), Some("0123456"));

        fs::remove_dir_all(root).unwrap();
    }
}
