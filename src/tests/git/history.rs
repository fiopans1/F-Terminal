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
