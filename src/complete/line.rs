use crate::term::InputLine;

/// Prompt terminators that are reliable enough to infer where the command
/// starts. The trailing-space variants are checked first; the bare glyphs are
/// common in Starship, Powerlevel10k and similar prompts.
const PROMPT_MARKERS: &[&str] = &[
    "$ ", "% ", "# ", "❯ ", "➜ ", "λ ", "» ", "→ ", "> ", "❯", "➜", "λ", "»", "→",
];

/// The command part of the current prompt line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPrefix {
    pub command: String,
}

/// Extract the text after the prompt from a terminal row.
///
/// `input.text` is expected to contain one character per column (wide-character
/// spacers are represented by spaces), so `col` can be used as a character
/// index. Returns `None` when the prompt cannot be recognized or when there is
/// text to the right of the cursor (right prompts, output, etc.).
pub fn extract_command(input: &InputLine) -> Option<CommandPrefix> {
    let chars: Vec<char> = input.text.chars().collect();
    let col = input.col.min(chars.len());

    let before: String = chars[..col].iter().collect();
    let mut best: Option<(usize, &str)> = None;
    for marker in PROMPT_MARKERS {
        if let Some(index) = before.find(marker)
            && best.is_none_or(|(best_index, best_marker)| {
                index < best_index || (index == best_index && marker.len() > best_marker.len())
            })
        {
            best = Some((index, marker));
        }
    }

    // cmd.exe and some PowerShell themes end the prompt with `>` and no
    // trailing space. Only accept it on drive/UNC prompt lines.
    let windows_prompt = before
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic())
        && before.as_bytes().get(1) == Some(&b':')
        || before.starts_with("\\\\");
    if best.is_none()
        && windows_prompt
        && let Some(index) = before.find('>')
    {
        let next = before[index + 1..].chars().next();
        if next.is_some_and(|character| character != ' ') {
            best = Some((index, ">"));
        }
    }

    let (index, marker) = best?;
    let command = before[index + marker.len()..].trim_start().to_owned();
    Some(CommandPrefix { command })
}

/// One shell word. `text` has quotes removed and backslash escapes resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub quoted: bool,
}

/// Tokenized command line. `current` points at the token being typed; when the
/// line ends with whitespace it points one past the last token and
/// `current_text` is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub tokens: Vec<Token>,
    pub current: usize,
    pub current_text: String,
    pub current_quoted: bool,
    pub ends_with_space: bool,
}

/// Split a command line using shell-like quoting rules.
pub fn parse_command(command: &str) -> ParsedCommand {
    let chars: Vec<char> = command.chars().collect();
    let mut tokens: Vec<Token> = Vec::new();
    let mut text = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut quoted = false;
    let mut start = 0;
    let mut in_token = false;
    let mut ends_with_space = false;

    for (index, character) in chars.iter().copied().enumerate() {
        if escaped {
            text.push(character);
            escaped = false;
            in_token = true;
            continue;
        }

        match quote {
            Some(delimiter) => {
                if character == '\\' && delimiter == '"' {
                    escaped = true;
                } else if character == delimiter {
                    quote = None;
                } else {
                    text.push(character);
                }
                in_token = true;
            }
            None => match character {
                ' ' | '\t' => {
                    if in_token {
                        tokens.push(Token {
                            text: std::mem::take(&mut text),
                            start,
                            end: index,
                            quoted,
                        });
                        in_token = false;
                        quoted = false;
                    }
                    ends_with_space = true;
                }
                '\'' | '"' => {
                    if !in_token {
                        start = index;
                    }
                    in_token = true;
                    quoted = true;
                    quote = Some(character);
                    ends_with_space = false;
                }
                '\\' => {
                    if !in_token {
                        start = index;
                    }
                    in_token = true;
                    escaped = true;
                    ends_with_space = false;
                }
                _ => {
                    if !in_token {
                        start = index;
                    }
                    text.push(character);
                    in_token = true;
                    ends_with_space = false;
                }
            },
        }
    }

    if in_token {
        tokens.push(Token {
            text,
            start,
            end: chars.len(),
            quoted,
        });
    }

    if !ends_with_space && !tokens.is_empty() {
        ends_with_space = false;
    }

    let current = if ends_with_space {
        tokens.len()
    } else {
        tokens.len().saturating_sub(1)
    };
    let current_text = tokens
        .get(current)
        .map(|token| token.text.clone())
        .unwrap_or_default();
    let current_quoted = tokens.get(current).is_some_and(|token| token.quoted);

    ParsedCommand {
        tokens,
        current,
        current_text,
        current_quoted,
        ends_with_space,
    }
}

#[cfg(test)]
mod tests {
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
}
