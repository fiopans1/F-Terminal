# Shell profiles

> [Documentation](README.md) · [Español](../es/perfiles-de-shell.md)

F-Terminal detects shells at startup and lets you choose one when creating a
terminal.

## Detection on macOS and Linux

Detection order:

1. `[shell] default` in `config.toml`, if defined.
2. `$SHELL`.
3. Entries in `/etc/shells`.
4. Fallbacks `/bin/zsh`, `/bin/bash`, `/bin/sh`.

Known shells (`zsh`, `bash`, `fish`, `sh`, `dash`, `ksh`, `tcsh`…) are launched
with `-l` so they become login shells and inherit the user's environment.

## Detection on Windows

They are detected in this order:

1. **PowerShell 7** (`pwsh.exe`), if present.
2. **Windows PowerShell** (`powershell.exe`).
3. **Command Prompt** (`cmd.exe`, `%COMSPEC%`).
4. **Git Bash** (`...\Git\bin\bash.exe --login -i`).
5. **MSYS2 Bash** and **Cygwin Bash**, if installed.
6. **WSL**: a generic profile and one profile per distribution detected with
   `wsl.exe -l -q`.

The default profile is the first one detected. If PowerShell 7 is not
installed, Windows PowerShell is used; if that is missing too, `cmd.exe`.

Windows notes:

- All shells run under the same **ConPTY**; the rest of the app is unchanged.
- Git Bash is launched with `bin\bash.exe` so it uses F-Terminal's ConPTY.
- WSL receives `--cd <path>` to translate the Windows cwd. Paths
  `\\wsl$\...` and `\\wsl.localhost\...` are converted to their Linux path.
- `WSLENV` propagates `TERM`, `COLORTERM`, `TERM_PROGRAM` and
  `MISE_IGNORED_CONFIG_PATHS` to the distribution. The latter prevents Linux
  `mise` from loading the Windows global config when the pane starts under
  `C:\Users\...` and fails because it is not trusted.
- On Windows, F-Terminal asks the shell itself for the path via `OSC 9;9`:
  PowerShell 7 and Windows PowerShell wrap their `prompt` function, `cmd.exe`
  uses `$E]9;9;$P` in `PROMPT`, and Git Bash/MSYS2/Cygwin add a
  `PROMPT_COMMAND`. This way splits, new tabs, the explorer and the Git footer
  follow the current `cd` even when the process does not update its cwd
  (PowerShell is the clearest case).
- If `git` is not on the process `PATH`, the usual Git for Windows, Scoop and
  Chocolatey installations are searched before giving up.
- WSL cannot read its Linux shell's cwd from Windows; WSL profiles keep using
  the last translated path known to F-Terminal.
- WSL and Git Bash profiles inherit `TERM=xterm-256color` and
  `COLORTERM=truecolor`.

## Shell selector

In the sidebar, next to the `+` button, there is a `▾` button:

- `+` creates a terminal with the default profile.
- `▾` opens the profile list inside the sidebar.
- `↑↓`/`Home`/`End` move the selection, `Enter` creates the terminal and `Esc`
  closes the selector.
- Clicking a profile creates a terminal with that shell.
- Shortcut: `Cmd+Shift+N` on macOS, `Ctrl+Shift+N` on Linux/Windows.

Splits inherit the focused pane's profile. The list shows the shell name and a
short label (`zsh`, `bash`, `cmd`, `PS`, `pwsh`, `WSL`).

## Default shell

The `[shell] default` field in `config.toml` lets you force a shell by path or
name. If omitted, the automatic detection described above is used.

---

[← Back to the index](README.md)
