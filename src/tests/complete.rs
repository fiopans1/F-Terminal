use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn context(command: &str) -> Context {
    Context::analyze(&line::parse_command(command))
}

fn scratch_dir() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "f-terminal-complete-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(path.join(".git").join("refs").join("heads")).unwrap();
    fs::write(
        path.join(".git").join("refs").join("heads").join("main"),
        "abc\n",
    )
    .unwrap();
    fs::create_dir_all(path.join(".git").join("refs").join("heads").join("feature")).unwrap();
    fs::write(
        path.join(".git")
            .join("refs")
            .join("heads")
            .join("feature")
            .join("login"),
        "def\n",
    )
    .unwrap();
    fs::write(path.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(path.join("main.rs"), "").unwrap();
    fs::write(path.join("lib.rs"), "").unwrap();
    path
}

#[test]
fn short_command_names_win_over_old_history() {
    let mut engine = Engine::default();
    engine.set_commands(vec![
        "ecdh_curve25519".to_owned(),
        "echo".to_owned(),
        "env".to_owned(),
    ]);
    engine.add_history("ecdh_curve25519 --old-command");

    let completion = engine.completion("ec", &std::env::temp_dir(), &Options::default());
    assert_eq!(completion.candidates[0].label, "echo");
    assert_eq!(completion.candidates[0].append, "ho");
}

#[test]
fn detects_git_subcommand_and_branch_positions() {
    assert_eq!(context("git ch").git, GitContext::Subcommand);
    assert_eq!(context("git ").git, GitContext::Subcommand);
    assert_eq!(context("git checkout ").git, GitContext::Branches);
    assert_eq!(context("git checkout fe").git, GitContext::Branches);
    assert_eq!(context("git checkout -b new").git, GitContext::NewBranch);
    assert_eq!(context("git commit ").git, GitContext::None);
}

#[test]
fn recognizes_command_position_after_wrappers_and_assignments() {
    assert!(context("car").current_is_command);
    assert!(context("FOO=bar git").current_is_command);
    assert!(context("sudo git").current_is_command);
    assert!(!context("git checkout").current_is_command);
}

#[test]
fn suppresses_paths_for_options_and_messages() {
    assert!(!context("git commit -m fix").path_allowed);
    assert!(!context("cargo build --release").path_allowed);
    assert!(context("cat src/").path_allowed);
    assert!(context("cd ").path_allowed);
    assert!(context("cd ").dirs_only);
}

#[test]
fn completes_history_and_git_subcommands() {
    let mut engine = Engine::default();
    engine.add_history("git checkout main");
    let completion = engine.completion("git ch", &std::env::temp_dir(), &Options::default());
    assert_eq!(completion.candidates[0].kind, Kind::Subcommand);
    assert_eq!(completion.candidates[0].append, "eckout");
    assert!(
        completion
            .candidates
            .iter()
            .any(|candidate| candidate.kind == Kind::History && candidate.append == "eckout main")
    );
}

#[test]
fn completes_paths_and_respects_directory_only_commands() {
    let root = scratch_dir();
    let mut engine = Engine::default();
    let completion = engine.completion("cd s", &root, &Options::default());
    assert_eq!(completion.candidates[0].kind, Kind::Path);
    assert_eq!(completion.candidates[0].append, "rc/");

    let completion = engine.completion("cat ma", &root, &Options::default());
    assert!(
        completion
            .candidates
            .iter()
            .any(|candidate| candidate.append == "in.rs")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn completes_git_branches_from_refs() {
    let root = scratch_dir();
    let mut engine = Engine::default();
    let completion = engine.completion("git checkout fe", &root, &Options::default());
    assert_eq!(completion.candidates[0].kind, Kind::Branch);
    assert_eq!(completion.candidates[0].append, "ature/login");
    assert_eq!(completion.candidates[0].detail, "branch");

    let completion = engine.completion("git checkout -b ne", &root, &Options::default());
    assert!(completion.candidates.iter().all(|candidate| {
        candidate.kind != Kind::Branch && candidate.kind != Kind::RemoteBranch
    }));
    fs::remove_dir_all(root).unwrap();
}
