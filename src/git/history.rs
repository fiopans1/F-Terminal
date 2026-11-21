use std::path::Path;

use anyhow::Result;

use crate::git::command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitInfo {
    pub hash: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub date: String,
    pub subject: String,
    pub refs: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistorySnapshot {
    pub commits: Vec<CommitInfo>,
}

pub fn load(repo: &Path, limit: usize) -> Result<HistorySnapshot> {
    let limit = limit.to_string();
    let output = command::run(
        repo,
        &[
            "log",
            "--all",
            "--topo-order",
            "--date=short",
            "-n",
            limit.as_str(),
            "--pretty=format:%H%x1f%P%x1f%an%x1f%ae%x1f%ad%x1f%D%x1f%s%x1e",
        ],
    )?;
    Ok(parse(&output.stdout))
}

pub fn parse(text: &str) -> HistorySnapshot {
    let mut commits = Vec::new();

    for record in text.split('\x1e') {
        let record = record.trim_matches(['\n', '\r']);
        if record.is_empty() {
            continue;
        }

        let mut fields = record.splitn(7, '\x1f');
        let Some(hash) = fields.next() else {
            continue;
        };
        let Some(parents) = fields.next() else {
            continue;
        };
        let Some(author) = fields.next() else {
            continue;
        };
        let Some(email) = fields.next() else {
            continue;
        };
        let Some(date) = fields.next() else {
            continue;
        };
        let Some(decorations) = fields.next() else {
            continue;
        };
        let subject = fields.next().unwrap_or_default();

        let refs = decorations
            .split(", ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();

        commits.push(CommitInfo {
            short: hash.chars().take(7).collect(),
            hash: hash.to_owned(),
            parents: parents.split_whitespace().map(str::to_owned).collect(),
            author: author.to_owned(),
            email: email.to_owned(),
            date: date.to_owned(),
            subject: subject.to_owned(),
            refs,
        });
    }

    HistorySnapshot { commits }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_records() {
        let text = "\
abc1234def\x1fparent1 parent2\x1fAda\x1fada@example.com\x1f2026-11-22\x1fHEAD -> main, origin/main\x1fFix footer\x1e\n\
def5678abc\x1f\x1fLinus\x1flinus@example.com\x1f2026-11-21\x1f\x1fInitial\x1e\n";
        let history = parse(text);
        assert_eq!(history.commits.len(), 2);
        assert_eq!(history.commits[0].short, "abc1234");
        assert_eq!(history.commits[0].parents.len(), 2);
        assert_eq!(history.commits[0].author, "Ada");
        assert_eq!(history.commits[0].refs, ["HEAD -> main", "origin/main"]);
        assert_eq!(history.commits[1].parents.len(), 0);
        assert_eq!(history.commits[1].subject, "Initial");
    }
}
