use super::*;

#[test]
fn parses_github_pull_requests() {
    let body = r#"[
            {
                "number": 42,
                "title": "Add search",
                "user": { "login": "octocat" },
                "head": { "ref": "feat/search" },
                "base": { "ref": "main" },
                "draft": false,
                "web_url": "https://github.com/o/r/pull/42",
                "updated_at": "2026-11-22T10:00:00Z"
            },
            {
                "number": 7,
                "title": "Draft work",
                "user": null,
                "head": { "ref": "wip" },
                "base": { "ref": "main" },
                "draft": true
            }
        ]"#;

    let pulls = parse_list(body).unwrap();
    assert_eq!(pulls.len(), 2);
    assert_eq!(pulls[0].id, 42);
    assert_eq!(pulls[0].id_label(), "#42");
    assert_eq!(pulls[0].author, "octocat");
    assert_eq!(pulls[0].source_branch, "feat/search");
    assert_eq!(pulls[0].web_url, "https://github.com/o/r/pull/42");
    assert_eq!(pulls[1].author, "ghost");
    assert!(pulls[1].draft);
    assert_eq!(pulls[0].list_title(), "#42 Add search");
}
