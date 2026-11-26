# Autocomplete

> [Documentation](README.md) · [Español](../es/autocompletado.md)

Each terminal offers suggestions while you type at the prompt:

- **History**: previously run commands, including the current session.
- **Commands**: executables from `PATH` and common builtins.
- **Git subcommands**: `checkout`, `switch`, `rebase`, etc.
- **Paths**: files and directories, relative, absolute and with `~`.
- **Git**: local and remote branches and tags, only when the subcommand
  accepts them (`git checkout`, `switch`, `merge`, `rebase`, `push`, `log`...).

The best suggestion appears as dimmed text behind the cursor:

| Action | Shortcut |
|---|---|
| Accept ghost text | `→` |
| Open/close the popup | `Ctrl+.`, `Ctrl+Space` or `Ctrl+Shift+Space` |
| Move the popup selection | `↑` / `↓` |
| Insert a candidate | `Tab` or `Enter` |
| Close the popup | `Esc` |

`Tab` without a popup is still sent to the shell, so shell-specific completion
for zsh/bash/fish or tools such as `docker` and `kubectl` is not lost.

The engine is deliberately lightweight:

- History in a single buffer with ranges, capped at 5,000 commands / 512 KB.
- `PATH` command index built once in the background.
- Paths: only the current token's directory is listed, with a 2 s cache.
- Git: direct reads of `refs/heads`, `refs/remotes`, `refs/tags` and
  `packed-refs`; no `git` process per keystroke.
- No index files and no writes to shell histories.

If the prompt uses an unrecognized format (`$ `, `% `, `# `, `❯`, `➜`, `λ`,
`»`, `→`, `> `) or the command spans multiple lines, no suggestions are shown.
In fish, inline history is skipped because fish already provides autosuggestions.

Autocomplete configuration is described in
[Configuration](configuration.md#full-example).

---

[← Back to the index](README.md)
