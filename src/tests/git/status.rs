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
