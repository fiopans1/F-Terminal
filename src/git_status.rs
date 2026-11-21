use std::path::{Path, PathBuf};
use std::process::Command;

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
/// Git is not available. Runs synchronously, so callers should use a worker
/// thread.
pub fn inspect(start: &Path) -> Option<GitStatus> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };

    let root = run_git(start, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root.trim());
    if root.as_os_str().is_empty() {
        return None;
    }

    let branch = run_git(&root, &["symbolic-ref", "--short", "-q", "HEAD"])
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            run_git(&root, &["rev-parse", "--short", "HEAD"]).map(|value| value.trim().to_owned())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "HEAD".to_owned());

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
    let output = Command::new("git")
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

    #[test]
    fn sums_numstat_and_ignores_binary_files() {
        let text = "10\t2\tsrc/a.rs\n3\t0\tsrc/b.rs\n-\t-\timage.png\n";
        assert_eq!(sum_numstat(text), (13, 2));
    }

    #[test]
    fn empty_numstat_is_zero() {
        assert_eq!(sum_numstat(""), (0, 0));
    }
}
