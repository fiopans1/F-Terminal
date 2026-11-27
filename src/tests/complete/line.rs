use super::*;

fn input(text: &str, col: usize) -> InputLine {
    InputLine {
        row: 3,
        col,
        text: text.to_owned(),
        right_limit: text.chars().count(),
    }
}

#[test]
fn extracts_common_prompts() {
    let cases = [
        ("user@host:~$ git st", "git st"),
        ("~/src ❯ cargo bu", "cargo bu"),
        ("root@host:/# ls", "ls"),
        ("PS C:\\src> git st", "git st"),
        ("C:\\src>git st", "git st"),
        ("❯ cd src", "cd src"),
    ];

    for (line, expected) in cases {
        let extracted = extract_command(&input(line, line.chars().count())).unwrap();
        assert_eq!(extracted.command, expected);
    }
}

#[test]
fn accepts_right_prompts_but_rejects_unknown_prompts() {
    let right_prompt = format!("user@host:~$ git st{}", " ".repeat(10));
    assert!(extract_command(&input(&right_prompt, 21)).is_some());
    assert!(extract_command(&input("user@host:~$ git st  12:30", 21)).is_some());
    assert!(extract_command(&input("git status", 10)).is_none());
}

#[test]
fn finds_user_host_prefixes() {
    assert_eq!(
        user_host_prefix_len("fiopans1@MacBook-Pro-de-Diego-3 F-Terminal % "),
        Some("fiopans1@MacBook-Pro-de-Diego-3 ".chars().count())
    );
    assert_eq!(
        user_host_prefix_len("user@host:~/git$ "),
        Some("user@host:".chars().count())
    );
    assert_eq!(
        user_host_prefix_len("root@127.0.0.1:~# "),
        Some("root@127.0.0.1:".chars().count())
    );
    assert_eq!(user_host_prefix_len("PS C:\\src> "), None);
    assert_eq!(user_host_prefix_len("➜ user@host ~/src"), None);
    assert_eq!(user_host_prefix_len("user@host"), Some(9));
}

#[test]
fn only_trims_rows_that_look_like_shell_prompts() {
    assert_eq!(
        prompt_user_prefix_len("user@host ~/repo % ls", 80),
        Some("user@host ".chars().count())
    );
    assert_eq!(
        prompt_user_prefix_len("user@host:~/repo$ ", 80),
        Some("user@host:".chars().count())
    );
    assert_eq!(prompt_user_prefix_len("user@host is up", 80), None);
    assert_eq!(prompt_user_prefix_len("PS C:\\src> ", 80), None);
}

#[test]
fn ignores_redirects_and_comments_after_the_prompt() {
    let redirect = extract_command(&input(
        "user@host:~$ echo hi > /tmp/out",
        "user@host:~$ echo hi > /tmp/out".chars().count(),
    ))
    .unwrap();
    assert_eq!(redirect.command, "echo hi > /tmp/out");

    let comment = extract_command(&input(
        "user@host:~$ echo hi # note",
        "user@host:~$ echo hi # note".chars().count(),
    ))
    .unwrap();
    assert_eq!(comment.command, "echo hi # note");
}

#[test]
fn keeps_trailing_space_for_empty_arguments() {
    let extracted = extract_command(&input("user@host:~$ cd ", 16)).unwrap();
    assert_eq!(extracted.command, "cd ");
}

#[test]
fn tokenizes_quotes_and_escapes() {
    let parsed = parse_command(r#"git commit -m "hello world" src/a\ b.rs"#);
    assert_eq!(parsed.tokens.len(), 5);
    assert_eq!(parsed.tokens[3].text, "hello world");
    assert!(parsed.tokens[3].quoted);
    assert_eq!(parsed.tokens[4].text, "src/a b.rs");
    assert_eq!(parsed.current, 4);
    assert_eq!(parsed.current_text, "src/a b.rs");
}

#[test]
fn detects_empty_current_token() {
    let parsed = parse_command("git checkout ");
    assert_eq!(parsed.current, 2);
    assert_eq!(parsed.current_text, "");
    assert!(parsed.ends_with_space);
    assert_eq!(parsed.tokens[0].text, "git");
    assert_eq!(parsed.tokens[1].text, "checkout");
}

#[test]
fn unterminated_quote_is_kept_for_path_completion() {
    let parsed = parse_command(r#"cd "My Doc"#);
    assert_eq!(parsed.current, 1);
    assert_eq!(parsed.current_text, "My Doc");
    assert!(parsed.current_quoted);
}
