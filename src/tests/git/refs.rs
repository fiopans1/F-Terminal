use super::*;

#[test]
fn parses_refs_and_stashes() {
    let text = "\
refs/heads/main\tmain\tabc123\torigin/main\t*\n\
refs/heads/feat/x\tfeat/x\tdef456\t\t\n\
refs/remotes/origin/main\torigin/main\tabc123\t\t\n\
refs/remotes/origin/HEAD\torigin\t\t\t\n\
refs/tags/v1.0\tv1.0\t111222\t\t\n";
    let refs = parse_refs(text);
    assert_eq!(refs.local_branches.len(), 2);
    assert_eq!(refs.local_branches[0].name, "main");
    assert!(refs.local_branches[0].current);
    assert_eq!(refs.remote_branches.len(), 1);
    assert_eq!(refs.tags.len(), 1);

    let stashes = parse_stashes("stash@{0}\tWIP on main: abc\nstash@{1}\tOld\n");
    assert_eq!(stashes.len(), 2);
    assert_eq!(stashes[0].index, 0);
    assert_eq!(stashes[0].message, "WIP on main: abc");
}
