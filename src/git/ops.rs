use std::path::Path;

use anyhow::{Result, bail};

use crate::git::command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Stage {
        paths: Vec<String>,
    },
    StageAll,
    Unstage {
        paths: Vec<String>,
    },
    UnstageAll,
    Discard {
        paths: Vec<String>,
        untracked: bool,
    },
    RevertFile {
        paths: Vec<String>,
        untracked: bool,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Checkout {
        branch: String,
        track: bool,
        local: String,
    },
    CreateBranch {
        name: String,
        checkout: bool,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    Fetch,
    Pull,
    Push {
        branch: String,
        set_upstream: bool,
    },
    StashSave {
        message: String,
        include_untracked: bool,
    },
    StashPop {
        index: usize,
    },
    StashDrop {
        index: usize,
    },
    Merge {
        branch: String,
    },
    AbortMerge,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stage { .. } | Self::StageAll => "Staging",
            Self::Unstage { .. } | Self::UnstageAll => "Unstaging",
            Self::Discard { .. } => "Discarding",
            Self::RevertFile { .. } => "Reverting",
            Self::Commit { .. } => "Committing",
            Self::Checkout { .. } => "Switching branches",
            Self::CreateBranch { .. } => "Creating branch",
            Self::DeleteBranch { .. } => "Deleting branch",
            Self::Fetch => "Fetching",
            Self::Pull => "Pulling",
            Self::Push { .. } => "Pushing",
            Self::StashSave { .. } => "Saving stash",
            Self::StashPop { .. } => "Applying stash",
            Self::StashDrop { .. } => "Dropping stash",
            Self::Merge { .. } => "Merging",
            Self::AbortMerge => "Aborting merge",
        }
    }
}

pub fn apply(repo: &Path, action: &Action) -> Result<String> {
    match action {
        Action::Stage { paths } => {
            if paths.is_empty() {
                command::run(repo, &["add", "-A"])?;
                return Ok("All changes staged".to_owned());
            }
            command::run_args(repo, &path_args(&["add", "--"], paths))?;
            Ok(format!("Staged: {}", files(paths.len())))
        }
        Action::StageAll => {
            command::run(repo, &["add", "-A"])?;
            Ok("All changes staged".to_owned())
        }
        Action::Unstage { paths } => {
            if paths.is_empty() {
                return Action::UnstageAll.apply_result(repo);
            }
            if command::run_args(repo, &path_args(&["restore", "--staged", "--"], paths)).is_err() {
                command::run_args(repo, &path_args(&["reset", "HEAD", "--"], paths))?;
            }
            Ok(format!("Unstaged: {}", files(paths.len())))
        }
        Action::UnstageAll => {
            if command::run(repo, &["restore", "--staged", "."]).is_err()
                && command::run(repo, &["reset", "HEAD", "--", "."]).is_err()
            {
                command::run(repo, &["rm", "-r", "--cached", "--", "."])?;
            }
            Ok("All changes unstaged".to_owned())
        }
        Action::Discard { paths, untracked } => {
            if paths.is_empty() {
                bail!("no files selected to discard");
            }
            if *untracked {
                command::run_args(repo, &path_args(&["clean", "-fd", "--"], paths))?;
            } else {
                command::run_args(repo, &path_args(&["restore", "--worktree", "--"], paths))?;
            }
            Ok(format!("Discarded: {}", files(paths.len())))
        }
        Action::RevertFile { paths, untracked } => {
            if paths.is_empty() {
                bail!("no files selected to revert");
            }
            if *untracked {
                command::run_args(repo, &path_args(&["clean", "-fd", "--"], paths))?;
                Ok(format!("Deleted: {}", files(paths.len())))
            } else {
                command::run_args(
                    repo,
                    &path_args(
                        &["restore", "--source=HEAD", "--staged", "--worktree", "--"],
                        paths,
                    ),
                )?;
                Ok(format!("Reverted: {}", files(paths.len())))
            }
        }
        Action::Commit { message, amend } => {
            if message.trim().is_empty() {
                bail!("the commit message is empty");
            }
            let mut args = vec!["commit", "-F", "-"];
            if *amend {
                args.push("--amend");
            }
            let output = command::run_with_stdin(repo, &args, message)?;
            Ok(first_line(
                &output,
                if *amend {
                    "Commit amended"
                } else {
                    "Commit created"
                },
            ))
        }
        Action::Checkout {
            branch,
            track,
            local,
        } => {
            if *track {
                if command::run(repo, &["switch", "-c", local, "--track", branch]).is_err() {
                    command::run(repo, &["checkout", "-b", local, "--track", branch])?;
                }
            } else if command::run(repo, &["switch", branch]).is_err() {
                command::run(repo, &["checkout", branch])?;
            }
            Ok(format!("On branch {branch}"))
        }
        Action::CreateBranch { name, checkout } => {
            validate_branch(repo, name)?;
            if *checkout {
                if command::run(repo, &["switch", "-c", name]).is_err() {
                    command::run(repo, &["checkout", "-b", name])?;
                }
                Ok(format!("Branch {name} created and checked out"))
            } else {
                command::run(repo, &["branch", name])?;
                Ok(format!("Branch {name} created"))
            }
        }
        Action::DeleteBranch { name, force } => {
            let flag = if *force { "-D" } else { "-d" };
            command::run(repo, &["branch", flag, name])?;
            Ok(format!("Branch {name} deleted"))
        }
        Action::Fetch => {
            command::run(repo, &["fetch", "--all", "--prune"])?;
            Ok("Fetch complete".to_owned())
        }
        Action::Pull => {
            command::run(repo, &["pull", "--ff-only"])?;
            Ok("Pull complete".to_owned())
        }
        Action::Push {
            branch,
            set_upstream,
        } => {
            if *set_upstream {
                command::run(repo, &["push", "-u", "origin", branch])?;
                Ok(format!("Push complete · upstream origin/{branch}"))
            } else {
                command::run(repo, &["push"])?;
                Ok("Push complete".to_owned())
            }
        }
        Action::StashSave {
            message,
            include_untracked,
        } => {
            let mut args = vec!["stash".to_owned(), "push".to_owned()];
            if *include_untracked {
                args.push("-u".to_owned());
            }
            if !message.trim().is_empty() {
                args.push("-m".to_owned());
                args.push(message.trim().to_owned());
            }
            command::run_args(repo, &args)?;
            Ok("Stash saved".to_owned())
        }
        Action::StashPop { index } => {
            command::run(repo, &["stash", "pop", &format!("stash@{{{index}}}")])?;
            Ok("Stash applied".to_owned())
        }
        Action::StashDrop { index } => {
            command::run(repo, &["stash", "drop", &format!("stash@{{{index}}}")])?;
            Ok("Stash dropped".to_owned())
        }
        Action::Merge { branch } => {
            command::run(repo, &["merge", branch])?;
            Ok("Merge complete".to_owned())
        }
        Action::AbortMerge => {
            command::run(repo, &["merge", "--abort"])?;
            Ok("Merge aborted".to_owned())
        }
    }
}

impl Action {
    fn apply_result(self, repo: &Path) -> Result<String> {
        apply(repo, &self)
    }
}

fn path_args(prefix: &[&str], paths: &[String]) -> Vec<String> {
    let mut args: Vec<String> = prefix.iter().map(|value| (*value).to_owned()).collect();
    args.extend(paths.iter().cloned());
    args
}

fn validate_branch(repo: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("the branch name is empty");
    }
    command::run(repo, &["check-ref-format", "--branch", name])?;
    Ok(())
}

fn files(count: usize) -> String {
    if count == 1 {
        "1 file".to_owned()
    } else {
        format!("{count} files")
    }
}

fn first_line(output: &command::Output, fallback: &str) -> String {
    output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{history, status};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reverts_modified_staged_untracked_and_renamed_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("f-terminal-git-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        command::run(&dir, &["init", "-q"]).unwrap();
        command::run(&dir, &["config", "user.name", "Test"]).unwrap();
        command::run(&dir, &["config", "user.email", "test@example.com"]).unwrap();
        fs::write(dir.join("modified.txt"), "base\n").unwrap();
        fs::write(dir.join("rename.txt"), "rename\n").unwrap();
        command::run(&dir, &["add", "."]).unwrap();
        command::run(&dir, &["commit", "-qm", "base"]).unwrap();

        fs::write(dir.join("modified.txt"), "changed\n").unwrap();
        fs::write(dir.join("staged.txt"), "staged new file\n").unwrap();
        command::run(&dir, &["add", "staged.txt"]).unwrap();
        fs::write(dir.join("untracked.txt"), "untracked\n").unwrap();
        command::run(&dir, &["mv", "rename.txt", "renamed.txt"]).unwrap();

        apply(
            &dir,
            &Action::RevertFile {
                paths: vec!["modified.txt".to_owned()],
                untracked: false,
            },
        )
        .unwrap();
        apply(
            &dir,
            &Action::RevertFile {
                paths: vec!["staged.txt".to_owned()],
                untracked: false,
            },
        )
        .unwrap();
        apply(
            &dir,
            &Action::RevertFile {
                paths: vec!["untracked.txt".to_owned()],
                untracked: true,
            },
        )
        .unwrap();
        apply(
            &dir,
            &Action::RevertFile {
                paths: vec!["renamed.txt".to_owned(), "rename.txt".to_owned()],
                untracked: false,
            },
        )
        .unwrap();

        let clean = status::load(&dir).unwrap();
        assert!(clean.entries.is_empty(), "{:?}", clean.entries);
        assert_eq!(
            fs::read_to_string(dir.join("modified.txt")).unwrap(),
            "base\n"
        );
        assert!(!dir.join("staged.txt").exists());
        assert!(!dir.join("untracked.txt").exists());
        assert_eq!(
            fs::read_to_string(dir.join("rename.txt")).unwrap(),
            "rename\n"
        );
        assert!(!dir.join("renamed.txt").exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stages_commits_and_creates_a_branch() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("f-terminal-git-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        command::run(&dir, &["init", "-q"]).unwrap();
        command::run(&dir, &["config", "user.name", "Test"]).unwrap();
        command::run(&dir, &["config", "user.email", "test@example.com"]).unwrap();
        fs::write(dir.join("file.txt"), "hello\n").unwrap();

        apply(&dir, &Action::StageAll).unwrap();
        let staged = status::load(&dir).unwrap();
        assert_eq!(staged.staged_count(), 1);

        apply(
            &dir,
            &Action::Commit {
                message: "initial".to_owned(),
                amend: false,
            },
        )
        .unwrap();
        let clean = status::load(&dir).unwrap();
        assert!(clean.entries.is_empty());

        let log = history::load(&dir, 10).unwrap();
        assert_eq!(log.commits[0].subject, "initial");

        apply(
            &dir,
            &Action::CreateBranch {
                name: "feat/test".to_owned(),
                checkout: true,
            },
        )
        .unwrap();
        let branch = status::load(&dir).unwrap();
        assert_eq!(branch.branch.as_deref(), Some("feat/test"));

        let _ = fs::remove_dir_all(dir);
    }
}
