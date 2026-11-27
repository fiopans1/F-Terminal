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
