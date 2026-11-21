pub mod command;
pub mod history;
pub mod ops;
pub mod refs;
pub mod status;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub use history::{CommitInfo, HistorySnapshot};
pub use refs::RefsSnapshot;
pub use status::{ChangeKind, StatusEntry, StatusSnapshot};

/// Everything the Git window needs for an initial render.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub repo: PathBuf,
    pub status: StatusSnapshot,
    pub refs: RefsSnapshot,
    pub history: HistorySnapshot,
}

pub fn load_snapshot(start: &Path) -> Result<Snapshot> {
    let repo = repository_root(start)?;
    let status = status::load(&repo)?;
    let refs = refs::load(&repo)?;
    let history = history::load(&repo, 300).unwrap_or_else(|error| {
        log::warn!("failed to load git history: {error:#}");
        HistorySnapshot::default()
    });

    Ok(Snapshot {
        repo,
        status,
        refs,
        history,
    })
}

pub fn repository_root(start: &Path) -> Result<PathBuf> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };

    let output = command::run(start, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("failed to inspect {}", start.display()))?;
    let root = output.stdout.trim();
    if root.is_empty() {
        bail!("not a git repository: {}", start.display());
    }
    Ok(PathBuf::from(root))
}
