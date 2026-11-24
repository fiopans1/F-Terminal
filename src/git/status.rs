use std::path::Path;

use anyhow::Result;

use crate::git::command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
}

impl ChangeKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::TypeChanged => "T",
            Self::Unmerged => "U",
            Self::Untracked => "?",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    pub original_path: Option<String>,
    pub index: char,
    pub worktree: char,
    pub kind: ChangeKind,
    pub staged: bool,
    pub unstaged: bool,
    pub conflicted: bool,
    pub untracked: bool,
}

impl StatusEntry {
    pub fn display_path(&self) -> String {
        match &self.original_path {
            Some(original) => format!("{original} → {}", self.path),
            None => self.path.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub detached: bool,
    pub entries: Vec<StatusEntry>,
    pub staged_count: usize,
    pub unstaged_count: usize,
}

impl StatusSnapshot {
    pub fn staged_count(&self) -> usize {
        self.staged_count
    }

    pub fn unstaged_count(&self) -> usize {
        self.unstaged_count
    }
}

pub fn load(repo: &Path) -> Result<StatusSnapshot> {
    let output = command::run(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    Ok(parse(&output.stdout))
}

pub fn parse(text: &str) -> StatusSnapshot {
    let mut snapshot = StatusSnapshot::default();
    let mut records = text.split('\0');

    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }

        if let Some(value) = record.strip_prefix("# branch.oid ") {
            let value = value.trim();
            if value != "(initial)" && !value.is_empty() {
                snapshot.head = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.head ") {
            let value = value.trim();
            if value == "(detached)" {
                snapshot.detached = true;
            } else if !value.is_empty() {
                snapshot.branch = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.upstream ") {
            let value = value.trim();
            if !value.is_empty() {
                snapshot.upstream = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.ab ") {
            let mut parts = value.split_whitespace();
            if let Some(ahead) = parts.next() {
                snapshot.ahead = ahead.trim_start_matches('+').parse().unwrap_or(0);
            }
            if let Some(behind) = parts.next() {
                snapshot.behind = behind.trim_start_matches('-').parse().unwrap_or(0);
            }
            continue;
        }
        if record.starts_with("# ") {
            continue;
        }

        let kind = record.as_bytes().first().copied();
        let entry = match kind {
            Some(b'1') => parse_ordinary(record, None),
            Some(b'2') => {
                let original = records.next().map(|value| value.to_owned());
                parse_ordinary(record, original)
            }
            Some(b'u') => parse_unmerged(record),
            Some(b'?') => parse_untracked(record),
            _ => None,
        };

        if let Some(entry) = entry {
            if entry.staged {
                snapshot.staged_count += 1;
            }
            if entry.unstaged {
                snapshot.unstaged_count += 1;
            }
            snapshot.entries.push(entry);
        }
    }

    snapshot
}

fn parse_ordinary(record: &str, original_path: Option<String>) -> Option<StatusEntry> {
    let is_rename = record.starts_with("2 ");
    let mut fields = record.splitn(if is_rename { 10 } else { 9 }, ' ');
    let record_kind = fields.next()?;
    let xy = fields.next()?;
    let _sub = fields.next()?;
    let _mh = fields.next()?;
    let _mi = fields.next()?;
    let _mw = fields.next()?;
    let _hh = fields.next()?;
    let _hi = fields.next()?;
    if is_rename {
        let _score = fields.next()?;
    }
    let path = fields.next()?.to_owned();
    let (index, worktree) = status_chars(xy)?;
    let staged = index != '.';

    Some(StatusEntry {
        path: path.clone(),
        original_path: if record_kind == "2" {
            original_path
        } else {
            None
        },
        index,
        worktree,
        kind: change_kind(index, worktree),
        staged,
        unstaged: worktree != '.',
        conflicted: false,
        untracked: false,
    })
}

fn parse_unmerged(record: &str) -> Option<StatusEntry> {
    let mut fields = record.splitn(11, ' ');
    fields.next()?;
    let xy = fields.next()?;
    let _sub = fields.next()?;
    for _ in 0..7 {
        fields.next()?;
    }
    let path = fields.next()?.to_owned();
    let (index, worktree) = status_chars(xy)?;

    Some(StatusEntry {
        path,
        original_path: None,
        index,
        worktree,
        kind: ChangeKind::Unmerged,
        staged: false,
        unstaged: true,
        conflicted: true,
        untracked: false,
    })
}

fn parse_untracked(record: &str) -> Option<StatusEntry> {
    let path = record.strip_prefix("? ")?.to_owned();
    Some(StatusEntry {
        path,
        original_path: None,
        index: '?',
        worktree: '?',
        kind: ChangeKind::Untracked,
        staged: false,
        unstaged: true,
        conflicted: false,
        untracked: true,
    })
}

fn status_chars(xy: &str) -> Option<(char, char)> {
    let mut chars = xy.chars();
    Some((chars.next()?, chars.next()?))
}

fn change_kind(index: char, worktree: char) -> ChangeKind {
    let code = if index != '.' && index != '?' {
        index
    } else {
        worktree
    };
    match code {
        'A' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' => ChangeKind::Copied,
        'T' => ChangeKind::TypeChanged,
        'U' => ChangeKind::Unmerged,
        _ => ChangeKind::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_v2_records() {
        let text = "\
# branch.oid abc123\0\
# branch.head main\0\
# branch.upstream origin/main\0\
# branch.ab +2 -1\0\
1 M. N... 100644 100644 100644 aaaa bbbb src/app.rs\0\
1 .M N... 100644 100644 100644 aaaa bbbb src/b.rs\0\
? new.rs\0\
2 R. N... 100644 100644 100644 aaaa bbbb R100 src/new name.rs\0src/old name.rs\0\
u UU N... 100644 100644 100644 100644 aaaa bbbb cccc conflict.rs\0";

        let status = parse(text);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 2);
        assert_eq!(status.behind, 1);
        assert!(!status.detached);
        assert_eq!(status.entries.len(), 5);
        assert_eq!(status.staged_count(), 2);
        assert_eq!(status.unstaged_count(), 3);

        let rename = status
            .entries
            .iter()
            .find(|entry| entry.kind == ChangeKind::Renamed)
            .unwrap();
        assert_eq!(rename.path, "src/new name.rs");
        assert_eq!(rename.original_path.as_deref(), Some("src/old name.rs"));

        let conflict = status
            .entries
            .iter()
            .find(|entry| entry.conflicted)
            .unwrap();
        assert_eq!(conflict.path, "conflict.rs");
        assert_eq!(conflict.kind, ChangeKind::Unmerged);
    }

    #[test]
    fn parses_detached_head() {
        let text = "# branch.oid abc123\0# branch.head (detached)\0";
        let status = parse(text);
        assert!(status.detached);
        assert_eq!(status.branch, None);
        assert_eq!(status.head.as_deref(), Some("abc123"));
    }
}
