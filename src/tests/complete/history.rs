use super::*;

#[test]
fn parses_zsh_extended_history() {
    let text = ": 1700000000:0;git status\n: 1700000001:0;cargo build --release\n";
    assert_eq!(parse_zsh(text), vec!["git status", "cargo build --release"]);
}

#[test]
fn skips_multiline_entries_instead_of_storing_fragments() {
    let text = ": 1:0;for i in 1 2; do\\\n  echo $i\\\ndone\n: 2:0;ls\n";
    assert_eq!(parse_zsh(text), vec!["ls"]);
}

#[test]
fn parses_fish_escapes() {
    let text = "- cmd: echo hello\\nworld\n- cmd: git status\n";
    assert_eq!(parse_fish(text), vec!["git status"]);
}

#[test]
fn ignores_bash_timestamps() {
    let text = "#1700000000\ngit status\n#1700000001\nls\n";
    assert_eq!(parse_lines(text), vec!["git status", "ls"]);
}

#[test]
fn suggestions_are_newest_first() {
    let history = History::from_commands(vec![
        "git status".into(),
        "git stash".into(),
        "git status".into(),
    ]);
    assert_eq!(history.len(), 2);
    assert_eq!(history.suggest("git st"), Some("git status"));
    assert_eq!(history.suggest("git status"), None);
    assert_eq!(history.suggest("nope"), None);
}

#[test]
fn adding_moves_duplicates_to_the_front() {
    let mut history = History::from_commands(vec!["one".into(), "two".into()]);
    history.add("one");
    assert_eq!(history.iter().collect::<Vec<_>>(), vec!["one", "two"]);
    assert_eq!(history.bytes(), 6);
}
