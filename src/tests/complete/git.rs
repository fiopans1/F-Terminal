use super::*;

#[test]
fn parses_packed_refs() {
    let text = "\
# pack-refs with: peeled fully-peeled sorted
abc123 refs/heads/main
def456 refs/heads/feature/login
111222 refs/remotes/origin/main
333444 refs/remotes/origin/HEAD
555666 refs/tags/v1.0
^777888
";
    let mut refs = Refs::default();
    parse_packed_refs(text, &mut refs);
    assert_eq!(refs.branches, vec!["main", "feature/login"]);
    assert_eq!(refs.remotes, vec!["origin/main", "origin/HEAD"]);
    assert_eq!(refs.tags, vec!["v1.0"]);

    refs.remotes.retain(|name| !name.ends_with("/HEAD"));
    assert_eq!(refs.remotes, vec!["origin/main"]);
}

#[test]
fn subcommand_lists_are_sorted() {
    let mut subcommands = SUBCOMMANDS.to_vec();
    subcommands.sort_unstable();
    assert_eq!(subcommands.as_slice(), SUBCOMMANDS);

    let mut branches = BRANCH_SUBCOMMANDS.to_vec();
    branches.sort_unstable();
    assert_eq!(branches.as_slice(), BRANCH_SUBCOMMANDS);
}
