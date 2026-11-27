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
    let dir = std::env::temp_dir().join(format!("f-terminal-git-{}-{unique}", std::process::id()));
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
    let dir = std::env::temp_dir().join(format!("f-terminal-git-{}-{unique}", std::process::id()));
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
