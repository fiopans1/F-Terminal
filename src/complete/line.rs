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

/// Prefix length for a row that looks like a shell prompt (starts with
/// `user@host` and contains a recognized prompt marker).
pub fn prompt_user_prefix_len(text: &str, cols: usize) -> Option<usize> {
    let prefix = user_host_prefix_len(text)?;
    if prefix == 0 || prefix >= cols {
        return None;
    }
    let rest = text
        .char_indices()
        .nth(prefix)
        .map(|(index, _)| &text[index..])
        .unwrap_or("");
    PROMPT_MARKERS
        .iter()
        .any(|marker| rest.contains(marker))
        .then_some(prefix)
}

/// Length in `char`s of a leading `user@host` prompt prefix, including an
/// optional trailing `:` or space. Returns `None` for prompts that do not
/// start with a username.
pub fn user_host_prefix_len(line: &str) -> Option<usize> {
    let first = line.chars().next()?;
    if !(first.is_alphanumeric() || matches!(first, '_' | '-' | '.')) {
        return None;
    }

    let token_end = line.find(char::is_whitespace).unwrap_or(line.len());
    let token = &line[..token_end];
    let at = token.find('@')?;
    if at == 0 {
        return None;
    }

    let mut host_end = at + 1;
    for (offset, character) in token[at + 1..].char_indices() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
            host_end = at + 1 + offset + character.len_utf8();
        } else {
            break;
        }
    }
    if host_end == at + 1 {
        return None;
    }

    let mut mask_end = host_end;
    if line[mask_end..].starts_with(':') {
        mask_end += 1;
    }
    if line[mask_end..].starts_with(' ') {
        mask_end += 1;
    }
    Some(line[..mask_end].chars().count())
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
#[path = "../tests/complete/line.rs"]
mod tests;
