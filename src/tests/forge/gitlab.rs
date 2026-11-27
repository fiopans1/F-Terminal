use super::*;

#[test]
fn encodes_nested_project_paths() {
    assert_eq!(
        encode_project("grupo/subgrupo/repo"),
        "grupo%2Fsubgrupo%2Frepo"
    );
}

#[test]
fn parses_gitlab_merge_requests() {
    let body = r#"[
            {
                "iid": 12,
                "title": "Add search",
                "author": { "username": "octocat" },
                "source_branch": "feat/search",
                "target_branch": "main",
                "draft": false,
                "web_url": "https://gitlab.com/g/r/-/merge_requests/12",
                "updated_at": "2026-11-22T10:00:00Z"
            },
            {
                "iid": 8,
                "title": "Draft work",
                "author": null,
                "source_branch": "wip",
                "target_branch": "main",
                "work_in_progress": true
            }
        ]"#;

    let changes = parse_list(body).unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].id, 12);
    assert_eq!(changes[0].id_label(), "!12");
    assert_eq!(changes[0].author, "octocat");
    assert_eq!(changes[0].source_branch, "feat/search");
    assert_eq!(changes[1].author, "ghost");
    assert!(changes[1].draft);
}

#[test]
fn builds_a_unified_diff_from_gitlab_changes() {
    let body = r#"{
            "overflow": false,
            "changes": [
                {
                    "old_path": "src/a.rs",
                    "new_path": "src/a.rs",
                    "diff": "@@ -1 +1 @@\n-old\n+new\n"
                },
                {
                    "old_path": "src/new.rs",
                    "new_path": "src/new.rs",
                    "new_file": true,
                    "diff": "@@ -0,0 +1 @@\n+hello\n"
                }
            ]
        }"#;

    let diff = build_diff(body).unwrap();
    assert!(diff.contains("diff --git a/src/a.rs b/src/a.rs"));
    assert!(diff.contains("new file mode 100644"));
    assert!(diff.contains("--- /dev/null"));
    assert!(diff.contains("@@ -1 +1 @@"));
}
