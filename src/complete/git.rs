use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(3);
const MAX_CACHED_REPOS: usize = 2;
const MAX_CACHED_ROOTS: usize = 8;
const MAX_REFS: usize = 2_000;
const MAX_REF_BYTES: usize = 256 * 1024;

pub const SUBCOMMANDS: &[&str] = &[
    "add",
    "am",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bundle",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "config",
    "describe",
    "diff",
    "fetch",
    "format-patch",
    "gc",
    "grep",
    "init",
    "log",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "revert",
    "rm",
    "shortlog",
    "show",
    "stash",
    "status",
    "submodule",
    "switch",
    "tag",
    "worktree",
];

pub const BRANCH_SUBCOMMANDS: &[&str] = &[
    "branch",
    "checkout",
    "cherry-pick",
    "diff",
    "fetch",
    "log",
    "merge",
    "pull",
    "push",
    "rebase",
    "reset",
    "restore",
    "revert",
    "show",
    "switch",
];

pub fn is_new_branch_flag(flag: &str) -> bool {
    matches!(flag, "-b" | "-B" | "-c" | "-C" | "--branch" | "--create")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refs {
    pub branches: Vec<String>,
    pub remotes: Vec<String>,
    pub tags: Vec<String>,
    pub current: Option<String>,
}

struct CacheEntry {
    root: PathBuf,
    loaded: Instant,
    refs: Arc<Refs>,
}

/// Reads loose and packed refs directly from `.git`, without spawning a
/// process. The result is cached briefly and shared between panes.
#[derive(Default)]
pub struct RefsCache {
    entries: Vec<CacheEntry>,
    roots: Vec<(PathBuf, PathBuf)>,
}

impl RefsCache {
    pub fn get(&mut self, cwd: &Path) -> Option<Arc<Refs>> {
        let root = if let Some((_, root)) = self.roots.iter().find(|(path, _)| path == cwd) {
            root.clone()
        } else {
            let root = find_repo_root(cwd)?;
            self.roots.push((cwd.to_path_buf(), root.clone()));
            if self.roots.len() > MAX_CACHED_ROOTS {
                self.roots.remove(0);
            }
            root
        };
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.root == root && entry.loaded.elapsed() < CACHE_TTL)
        {
            return Some(entry.refs.clone());
        }

        let refs = Arc::new(load_refs(&root).unwrap_or_default());
        self.entries.retain(|entry| entry.root != root);
        self.entries.push(CacheEntry {
            root,
            loaded: Instant::now(),
            refs: refs.clone(),
        });
        if self.entries.len() > MAX_CACHED_REPOS
            && let Some((oldest, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.loaded)
        {
            self.entries.remove(oldest);
        }
        Some(refs)
    }
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?
    } else {
        start
    };
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn load_refs(root: &Path) -> Option<Refs> {
    let git_dir = resolve_git_dir(root)?;
    let common_dir = resolve_common_dir(&git_dir);
    let mut refs = Refs::default();

    collect_loose(
        &common_dir.join("refs").join("heads"),
        "",
        &mut refs.branches,
    );
    collect_loose(
        &common_dir.join("refs").join("remotes"),
        "",
        &mut refs.remotes,
    );
    collect_loose(&common_dir.join("refs").join("tags"), "", &mut refs.tags);

    if let Ok(text) = fs::read_to_string(common_dir.join("packed-refs")) {
        parse_packed_refs(&text, &mut refs);
    }

    refs.remotes.retain(|name| !name.ends_with("/HEAD"));
    sort_unique(&mut refs.branches);
    sort_unique(&mut refs.remotes);
    sort_unique(&mut refs.tags);
    refs.branches.truncate(MAX_REFS);
    refs.remotes.truncate(MAX_REFS);
    refs.tags.truncate(MAX_REFS);

    let mut budget = MAX_REF_BYTES;
    trim_refs(&mut refs.branches, &mut budget);
    trim_refs(&mut refs.remotes, &mut budget);
    trim_refs(&mut refs.tags, &mut budget);

    if let Ok(head) = fs::read_to_string(git_dir.join("HEAD"))
        && let Some(name) = head.trim().strip_prefix("ref: refs/heads/")
    {
        refs.current = Some(name.to_owned());
    }

    Some(refs)
}

fn resolve_git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let path = text.trim().strip_prefix("gitdir: ")?;
    let path = PathBuf::from(path);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let Ok(text) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let path = PathBuf::from(text.trim());
    if path.is_absolute() {
        path
    } else {
        git_dir.join(path)
    }
}

fn collect_loose(directory: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect_loose(&path, &name, out);
        } else if path.is_file() {
            out.push(name);
        }
    }
}

fn parse_packed_refs(text: &str, refs: &mut Refs) {
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(_hash) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if let Some(name) = name.strip_prefix("refs/heads/") {
            refs.branches.push(name.to_owned());
        } else if let Some(name) = name.strip_prefix("refs/remotes/") {
            refs.remotes.push(name.to_owned());
        } else if let Some(name) = name.strip_prefix("refs/tags/") {
            refs.tags.push(name.to_owned());
        }
    }
}

fn sort_unique(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| !value.is_empty() && seen.insert(value.clone()));
    values.sort_unstable();
}

fn trim_refs(values: &mut Vec<String>, budget: &mut usize) {
    values.retain(|value| {
        let cost = value.len() + 1;
        if cost <= *budget {
            *budget -= cost;
            true
        } else {
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_packed_refs() {
        let text = "\
# pack-refs with: peeled fully-peeled sorted
abc123 refs/heads/main
def456 refs/heads/feature/login
111222 refs/remotes/origin/main
333444 refs/remotes/origin/HEAD
555666 refs/tags/v1.0
^777888
";
        let mut refs = Refs::default();
        parse_packed_refs(text, &mut refs);
        assert_eq!(refs.branches, vec!["main", "feature/login"]);
        assert_eq!(refs.remotes, vec!["origin/main", "origin/HEAD"]);
        assert_eq!(refs.tags, vec!["v1.0"]);

        refs.remotes.retain(|name| !name.ends_with("/HEAD"));
        assert_eq!(refs.remotes, vec!["origin/main"]);
    }

    #[test]
    fn subcommand_lists_are_sorted() {
        let mut subcommands = SUBCOMMANDS.to_vec();
        subcommands.sort_unstable();
        assert_eq!(subcommands.as_slice(), SUBCOMMANDS);

        let mut branches = BRANCH_SUBCOMMANDS.to_vec();
        branches.sort_unstable();
        assert_eq!(branches.as_slice(), BRANCH_SUBCOMMANDS);
    }
}
