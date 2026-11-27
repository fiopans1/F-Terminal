use std::path::Path;

use anyhow::Result;

use crate::git::command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub full: String,
    pub hash: String,
    pub upstream: Option<String>,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagInfo {
    pub name: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashInfo {
    pub index: usize,
    pub name: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefsSnapshot {
    pub local_branches: Vec<BranchInfo>,
    pub remote_branches: Vec<BranchInfo>,
    pub tags: Vec<TagInfo>,
    pub stashes: Vec<StashInfo>,
}

pub fn load(repo: &Path) -> Result<RefsSnapshot> {
    let output = command::run(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(refname:short)%09%(objectname)%09%(upstream:short)%09%(HEAD)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;

    let mut refs = parse_refs(&output.stdout);
    if let Ok(stashes) = command::run(repo, &["stash", "list", "--format=%gd%09%s"]) {
        refs.stashes = parse_stashes(&stashes.stdout);
    }
    Ok(refs)
}

pub fn parse_refs(text: &str) -> RefsSnapshot {
    let mut refs = RefsSnapshot::default();

    for line in text.lines() {
        let mut fields = line.splitn(5, '\t');
        let Some(full) = fields.next() else { continue };
        let Some(name) = fields.next() else { continue };
        let Some(hash) = fields.next() else {
            continue;
        };
        let upstream = fields.next().unwrap_or_default();
        let head = fields.next().unwrap_or_default();

        if full.starts_with("refs/heads/") {
            refs.local_branches.push(BranchInfo {
                name: name.to_owned(),
                full: full.to_owned(),
                hash: hash.to_owned(),
                upstream: (!upstream.is_empty()).then(|| upstream.to_owned()),
                current: head == "*",
            });
        } else if full.starts_with("refs/remotes/") {
            if full.ends_with("/HEAD") {
                continue;
            }
            refs.remote_branches.push(BranchInfo {
                name: name.to_owned(),
                full: full.to_owned(),
                hash: hash.to_owned(),
                upstream: None,
                current: false,
            });
        } else if full.starts_with("refs/tags/") {
            refs.tags.push(TagInfo {
                name: name.to_owned(),
                hash: hash.to_owned(),
            });
        }
    }

    refs.local_branches
        .sort_by(|a, b| b.current.cmp(&a.current).then(a.name.cmp(&b.name)));
    refs.remote_branches.sort_by(|a, b| a.name.cmp(&b.name));
    refs.tags.sort_by(|a, b| a.name.cmp(&b.name));
    refs
}

pub fn parse_stashes(text: &str) -> Vec<StashInfo> {
    let mut stashes = Vec::new();

    for line in text.lines() {
        let mut fields = line.splitn(2, '\t');
        let Some(name) = fields.next() else { continue };
        let message = fields.next().unwrap_or_default();
        let index = name
            .split_once('{')
            .and_then(|(_, rest)| rest.split_once('}'))
            .and_then(|(value, _)| value.parse().ok())
            .unwrap_or(stashes.len());
        stashes.push(StashInfo {
            index,
            name: name.to_owned(),
            message: message.to_owned(),
        });
    }

    stashes
}

#[cfg(test)]
#[path = "../tests/git/refs.rs"]
mod tests;
