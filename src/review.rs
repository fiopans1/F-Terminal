use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::forge::{self, ForgeKind};

const MAX_DIFF_LINES: usize = 50_000;
const MAX_UNTRACKED_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewKind {
    Working,
    Branch { base: String },
    Commit { hash: String },
    RemoteChange { forge: ForgeKind, id: u64 },
}

#[derive(Clone, Debug)]
pub struct ReviewSummary {
    pub title: String,
    pub subtitle: String,
    pub kind: ReviewKind,
}

pub use crate::forge::RemoteChange;

impl RemoteChange {
    pub fn to_review(&self) -> ReviewSummary {
        let draft = if self.draft { " · draft" } else { "" };
        ReviewSummary {
            title: self.list_title(),
            subtitle: format!(
                "@{} · {} → {}{}",
                self.author, self.source_branch, self.target_branch, draft
            ),
            kind: ReviewKind::RemoteChange {
                forge: self.forge,
                id: self.id,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Add,
    Remove,
    Hunk,
    Meta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Binary,
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
    pub binary: bool,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug)]
pub struct ReviewDetail {
    pub summary: ReviewSummary,
    pub files: Vec<FileDiff>,
    pub additions: usize,
    pub deletions: usize,
}

/// State for the Code Review window.
pub struct CodeReview {
    pub repo: PathBuf,
    pub reviews: Vec<ReviewSummary>,
    pub selected: usize,
    pub list_offset: usize,
    pub detail: Option<ReviewDetail>,
    pub loading: bool,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub error: Option<String>,
    pub generation: u64,
    pub forge: Option<ForgeKind>,
    pub pull_requests: Vec<RemoteChange>,
    pub prs_loading: bool,
    pub prs_loaded: bool,
    pub prs_error: Option<String>,
}

impl CodeReview {
    pub fn open(start: PathBuf) -> Self {
        match repository_root(&start) {
            Ok(repo) => {
                let (reviews, error) = match load_reviews(&repo) {
                    Ok(reviews) => (reviews, None),
                    Err(error) => (Vec::new(), Some(format!("{error:#}"))),
                };
                let forge = forge::detect(&repo).ok().map(|remote| remote.kind);

                Self {
                    repo,
                    reviews,
                    selected: 0,
                    list_offset: 0,
                    detail: None,
                    loading: false,
                    scroll_row: 0,
                    scroll_col: 0,
                    error,
                    generation: 0,
                    forge,
                    pull_requests: Vec::new(),
                    prs_loading: false,
                    prs_loaded: false,
                    prs_error: None,
                }
            }
            Err(error) => Self {
                repo: start,
                reviews: Vec::new(),
                selected: 0,
                list_offset: 0,
                detail: None,
                loading: false,
                scroll_row: 0,
                scroll_col: 0,
                error: Some(format!("{error:#}")),
                generation: 0,
                forge: None,
                pull_requests: Vec::new(),
                prs_loading: false,
                prs_loaded: false,
                prs_error: None,
            },
        }
    }

    pub fn current_summary(&self) -> Option<&ReviewSummary> {
        self.reviews.get(self.selected)
    }

    pub fn start_loading_summary(
        &mut self,
        generation: u64,
        summary: ReviewSummary,
    ) -> (PathBuf, ReviewSummary) {
        self.generation = generation;
        self.loading = true;
        self.detail = None;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.error = None;
        (self.repo.clone(), summary)
    }

    pub fn apply_pull_requests(&mut self, result: std::result::Result<Vec<RemoteChange>, String>) {
        self.prs_loading = false;
        self.prs_loaded = true;
        match result {
            Ok(pull_requests) => {
                self.pull_requests = pull_requests;
                self.prs_error = None;
            }
            Err(error) => self.prs_error = Some(error),
        }
    }

    pub fn apply_detail(
        &mut self,
        generation: u64,
        result: std::result::Result<ReviewDetail, String>,
    ) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        match result {
            Ok(detail) => {
                self.scroll_row = 0;
                self.scroll_col = 0;
                self.detail = Some(detail);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn back_to_list(&mut self) {
        self.generation = 0;
        self.detail = None;
        self.loading = false;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.error = None;
    }

    pub fn refresh(&mut self) {
        let selected_kind = self.current_summary().map(|summary| summary.kind.clone());
        self.generation = 0;
        self.error = None;
        self.detail = None;
        self.loading = false;
        self.scroll_row = 0;
        self.scroll_col = 0;

        match load_reviews(&self.repo) {
            Ok(reviews) => {
                self.reviews = reviews;
                self.selected = selected_kind
                    .and_then(|kind| self.reviews.iter().position(|review| review.kind == kind))
                    .unwrap_or(0)
                    .min(self.reviews.len().saturating_sub(1));
                self.list_offset = 0;
            }
            Err(error) => {
                self.reviews.clear();
                self.error = Some(format!("{error:#}"));
            }
        }
    }
}

pub fn load_detail(repo: &Path, summary: &ReviewSummary) -> Result<ReviewDetail> {
    let mut diff = match &summary.kind {
        ReviewKind::Working => {
            let tracked =
                git(repo, &["diff", "--no-color", "--no-ext-diff", "HEAD"]).unwrap_or_default();
            let mut combined = tracked;
            combined.push_str(&untracked_diff(repo)?);
            combined
        }
        ReviewKind::Branch { base } => {
            let range = format!("{base}...HEAD");
            git(repo, &["diff", "--no-color", "--no-ext-diff", &range])?
        }
        ReviewKind::Commit { hash } => git(
            repo,
            &[
                "show",
                "--no-color",
                "--no-ext-diff",
                "--format=",
                "--patch",
                hash,
            ],
        )?,
        ReviewKind::RemoteChange { forge, id } => forge::load_change_diff(repo, *forge, *id)?,
    };

    if diff.lines().count() > MAX_DIFF_LINES {
        let mut truncated: String = diff
            .lines()
            .take(MAX_DIFF_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        truncated.push_str("\n... diff truncated ...\n");
        diff = truncated;
    }

    let detail = parse_diff(&diff, summary.clone());
    Ok(detail)
}

fn repository_root(start: &Path) -> Result<PathBuf> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };

    let output = git_at(start, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("failed to inspect {}", start.display()))?;
    let root = output.trim();
    if root.is_empty() {
        bail!("not a git repository: {}", start.display());
    }
    Ok(PathBuf::from(root))
}

fn load_reviews(repo: &Path) -> Result<Vec<ReviewSummary>> {
    let mut reviews = Vec::new();

    let status =
        git(repo, &["status", "--porcelain=v1", "--untracked-files=all"]).unwrap_or_default();
    if !status.trim().is_empty() {
        let untracked = status
            .lines()
            .filter(|line| line.starts_with("?? "))
            .count();
        let stat = git(repo, &["diff", "--shortstat", "HEAD"]).unwrap_or_default();
        let stat = stat.trim();
        let subtitle = match (stat.is_empty(), untracked) {
            (false, 0) => stat.to_owned(),
            (false, count) => format!("{stat} · {count} untracked"),
            (true, 0) => "uncommitted changes".to_owned(),
            (true, count) => format!("{count} untracked file(s)"),
        };
        reviews.push(ReviewSummary {
            title: "Working changes".to_owned(),
            subtitle,
            kind: ReviewKind::Working,
        });
    }

    if let Some(base) = default_base(repo) {
        let range = format!("{base}...HEAD");
        let stat = git(repo, &["diff", "--shortstat", &range]).unwrap_or_default();
        let stat = stat.trim();
        if !stat.is_empty() {
            reviews.push(ReviewSummary {
                title: format!("Branch vs {base}"),
                subtitle: stat.to_owned(),
                kind: ReviewKind::Branch { base },
            });
        }
    }

    let log = git(
        repo,
        &[
            "log",
            "--format=%H%x09%h%x09%s%x09%ad",
            "--date=short",
            "-n",
            "15",
        ],
    )
    .unwrap_or_default();

    for line in log.lines() {
        let mut fields = line.splitn(4, '\t');
        let Some(hash) = fields.next() else { continue };
        let Some(short) = fields.next() else { continue };
        let Some(subject) = fields.next() else {
            continue;
        };
        let date = fields.next().unwrap_or_default();

        reviews.push(ReviewSummary {
            title: subject.to_owned(),
            subtitle: format!("{short} · {date}"),
            kind: ReviewKind::Commit {
                hash: hash.to_owned(),
            },
        });
    }

    Ok(reviews)
}

fn default_base(repo: &Path) -> Option<String> {
    if let Ok(output) = git(
        repo,
        &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
    ) {
        let name = output.trim().trim_start_matches("origin/");
        if !name.is_empty() {
            return Some(name.to_owned());
        }
    }

    for candidate in ["main", "master"] {
        if git(repo, &["rev-parse", "--verify", candidate]).is_ok() {
            return Some(candidate.to_owned());
        }
    }

    None
}

fn untracked_diff(repo: &Path) -> Result<String> {
    let status = git(repo, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    let mut diff = String::new();

    for line in status.lines() {
        let Some(path) = line.strip_prefix("?? ") else {
            continue;
        };
        let file = repo.join(path);
        let Ok(bytes) = fs::read(&file) else {
            continue;
        };
        if bytes.len() > MAX_UNTRACKED_BYTES || bytes.contains(&0) {
            diff.push_str(&format!(
                "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+[untracked file omitted]\n"
            ));
            continue;
        }

        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            continue;
        }

        diff.push_str(&format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
            lines.len()
        ));
        for line in lines {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
    }

    Ok(diff)
}

fn parse_diff(diff: &str, summary: ReviewSummary) -> ReviewDetail {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current: Option<FileDiff> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;
    let mut parsed_lines = 0usize;
    let mut truncated = false;

    for raw in diff.lines() {
        if parsed_lines >= MAX_DIFF_LINES {
            truncated = true;
            break;
        }

        if raw.starts_with("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }

            let path = raw
                .split_whitespace()
                .last()
                .map(|value| value.trim_start_matches("b/"))
                .unwrap_or("file")
                .to_owned();

            current = Some(FileDiff {
                path,
                old_path: None,
                status: FileStatus::Modified,
                additions: 0,
                deletions: 0,
                binary: false,
                lines: Vec::new(),
            });
            continue;
        }

        let Some(file) = current.as_mut() else {
            continue;
        };

        if raw.starts_with("@@") {
            let (old_start, new_start) = parse_hunk_header(raw);
            old_line = old_start;
            new_line = new_start;
            file.lines.push(DiffLine {
                kind: DiffLineKind::Hunk,
                text: raw.to_owned(),
                old: None,
                new: None,
            });
            parsed_lines += 1;
            continue;
        }

        if raw.starts_with("+++")
            || raw.starts_with("---")
            || raw.starts_with("index ")
            || raw.starts_with("similarity index")
            || raw.starts_with("dissimilarity index")
            || raw.starts_with("old mode")
            || raw.starts_with("new mode")
        {
            continue;
        }

        if raw.starts_with("new file mode") {
            file.status = FileStatus::Added;
            continue;
        }

        if raw.starts_with("deleted file mode") {
            file.status = FileStatus::Deleted;
            continue;
        }

        if let Some(old_path) = raw.strip_prefix("rename from ") {
            file.old_path = Some(old_path.to_owned());
            file.status = FileStatus::Renamed;
            continue;
        }

        if let Some(new_path) = raw.strip_prefix("rename to ") {
            file.path = new_path.to_owned();
            file.status = FileStatus::Renamed;
            continue;
        }

        if raw.starts_with("Binary files") {
            file.binary = true;
            file.status = FileStatus::Binary;
            continue;
        }

        if let Some(text) = raw.strip_prefix('+') {
            file.additions += 1;
            file.lines.push(DiffLine {
                kind: DiffLineKind::Add,
                text: text.to_owned(),
                old: None,
                new: Some(new_line),
            });
            new_line += 1;
            parsed_lines += 1;
            continue;
        }

        if let Some(text) = raw.strip_prefix('-') {
            file.deletions += 1;
            file.lines.push(DiffLine {
                kind: DiffLineKind::Remove,
                text: text.to_owned(),
                old: Some(old_line),
                new: None,
            });
            old_line += 1;
            parsed_lines += 1;
            continue;
        }

        if let Some(text) = raw.strip_prefix(' ') {
            file.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: text.to_owned(),
                old: Some(old_line),
                new: Some(new_line),
            });
            old_line += 1;
            new_line += 1;
            parsed_lines += 1;
            continue;
        }

        if raw.starts_with('\\') {
            file.lines.push(DiffLine {
                kind: DiffLineKind::Meta,
                text: raw.to_owned(),
                old: None,
                new: None,
            });
            parsed_lines += 1;
        }
    }

    if let Some(file) = current.take() {
        files.push(file);
    }

    if truncated && let Some(file) = files.last_mut() {
        file.lines.push(DiffLine {
            kind: DiffLineKind::Meta,
            text: "... diff truncated ...".to_owned(),
            old: None,
            new: None,
        });
    }

    let additions = files.iter().map(|file| file.additions).sum();
    let deletions = files.iter().map(|file| file.deletions).sum();

    ReviewDetail {
        summary,
        files,
        additions,
        deletions,
    }
}

fn parse_hunk_header(header: &str) -> (u32, u32) {
    let mut old = 1;
    let mut new = 1;

    for part in header.split_whitespace() {
        if let Some(value) = part.strip_prefix('-') {
            old = value
                .split(',')
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
        } else if let Some(value) = part.strip_prefix('+') {
            new = value
                .split(',')
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
        }
    }

    (old, new)
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    git_at(repo, args)
}

fn git_at(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("color.ui=false")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .output()
        .context("failed to run git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_unified_diff() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 123..456 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@ fn main() {
 fn main() {
-    old();
+    new();
+    extra();
 }
";
        let detail = parse_diff(
            diff,
            ReviewSummary {
                title: "test".into(),
                subtitle: String::new(),
                kind: ReviewKind::Working,
            },
        );

        assert_eq!(detail.additions, 2);
        assert_eq!(detail.deletions, 1);
        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.files[0].path, "src/lib.rs");
        assert_eq!(detail.files[0].status, FileStatus::Modified);
        assert!(
            detail.files[0]
                .lines
                .iter()
                .any(|line| line.kind == DiffLineKind::Hunk)
        );

        let add = detail.files[0]
            .lines
            .iter()
            .find(|line| line.kind == DiffLineKind::Add)
            .unwrap();
        assert_eq!(add.new, Some(2));

        let remove = detail.files[0]
            .lines
            .iter()
            .find(|line| line.kind == DiffLineKind::Remove)
            .unwrap();
        assert_eq!(remove.old, Some(2));
    }
}
