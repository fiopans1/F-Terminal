use super::*;

#[test]
fn builtins_are_sorted_and_unique() {
    let mut sorted = BUILTINS.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.as_slice(), BUILTINS);

    let mut seen = HashSet::new();
    for builtin in BUILTINS {
        assert!(seen.insert(*builtin));
    }
}
