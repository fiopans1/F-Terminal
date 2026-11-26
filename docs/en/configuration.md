# Configuration

> [Documentation](README.md) · [Español](../es/configuracion.md)

F-Terminal uses a single `config.toml` file. It is created automatically on
first launch from a commented template, with `600` permissions on Unix.

| Platform | Path |
|---|---|
| macOS / Linux | `~/.config/f-terminal/config.toml` |
| Linux with `XDG_CONFIG_HOME` | `$XDG_CONFIG_HOME/f-terminal/config.toml` |
| Windows | `%APPDATA%\f-terminal\config.toml` |

The `F_TERMINAL_CONFIG` environment variable lets you use another path. The
font index is stored in the system cache directory
(`~/Library/Caches/f-terminal` on macOS, `$XDG_CACHE_HOME/f-terminal` or
`~/.cache/f-terminal` on Linux, `%LOCALAPPDATA%\f-terminal\cache` on Windows);
`F_TERMINAL_CACHE_DIR` lets you change it.

`Cmd+Shift+P` / `Ctrl+Shift+P` opens `config.toml` in the built-in editor.
Changes are applied live as soon as the file is saved: theme, font sizes,
margins, scrollback, default shell profile, agents and the rest of the behavior
options. Structural changes —`font_family` and `render.mode`— still require
restarting the app; everything else is applied without losing open sessions.

If the TOML cannot be read or parsed during a reload, the previous
configuration is kept (it does not reset to defaults) and a warning is written
to the log. On startup, an invalid file does fall back to defaults. Out-of-range
numbers are clamped; an unknown `render.mode` falls back to `isolated` and an
unknown theme falls back to Foundry.

## Full example

```toml
[appearance]
# font_family = "SF Mono"
font_size = 14.0
line_height = 1.3
# No frame: the terminal reaches the panel edges.
padding = 0.0
# Extra left margin for terminal text.
padding_left = 8.0
# false hides the user@host prefix from the prompt (on any OS).
show_user_in_prompt = true
sidebar_width = 210.0
ui_font_size = 12.5
# The default depends on the platform:
# false on macOS, true on Linux/Windows.
option_as_meta = false
theme = "foundry"

[terminal]
# History per pane in the active session.
scrollback_lines = 6000
# History per pane in hidden sessions.
hidden_scrollback_lines = 3000
# Global scrollback budget in MB. 0 disables it.
scrollback_budget_mb = 0
# Autocomplete. Disable the sources you do not want.
completions = true
completion_history = true
completion_commands = true
completion_paths = true
completion_git = true
# Minimum characters to show ghost text.
completion_min_chars = 2

[render]
# "shared" shares the GPU and text atlas between windows.
# "isolated" creates separate resources for each window.
mode = "shared"

[session]
# Restores tabs, splits, directories and geometry on open.
restore = true
# Also restores each pane's scrollback.
restore_scrollback = true

[agents]
# Discovers agents and responds to OSC 7501.
enabled = true
# Includes agents from other terminals/IDEs (read-only).
show_external = true
# Process scan interval, in milliseconds.
poll_interval_ms = 2000

[shell]
# default = "zsh"

[forge.github]
# token = "github_pat_..."

[forge.gitlab]
# token = "glpat-..."
```

## Left margin

`padding_left` adds space only to the left of the terminal grid (8 px by
default), so text is not glued to the sidebar. The other sides remain
borderless; `padding` still controls an inset on all four sides if desired.

## Visual identity

**Foundry** is F-Terminal's own identity: graphite surfaces with a touch of
petrol green and a warm brass accent. Brass marks the primary action, focus and
the current branch; petrol green is reserved for work in progress and
informational states. It is not a GitHub Dark variant: the warm/cool contrast
is meant to make the app recognizable at a glance.

The shell of the Git, Code Review and Agents windows shares heights (56 px
header, 28 px footer), a minimum type scale of 11 px, 30 px buttons, visible
focus and Spanish copy. Empty states explain what to do next, and long lists
show a scrollbar.

## Themes

`theme` selects the complete palette (terminal, sidebar, diffs and Code
Review):

| Value | Theme |
|---|---|
| `foundry` (default) | Foundry |
| `github-dark` | GitHub Dark |
| `tokyo-night` | Tokyo Night |
| `catppuccin-mocha` | Catppuccin Mocha |
| `dracula` | Dracula |
| `nord` | Nord |
| `gruvbox-dark` | Gruvbox Dark |
| `one-dark` | One Dark |
| `solarized-dark` | Solarized Dark |
| `kanagawa` | Kanagawa |

Hyphens or underscores are accepted, along with some aliases (`gruvbox`,
`onedark`, `solarized`, `github`). An unknown name falls back to Foundry and a
warning is written to the log.

## `user@host` prefix

With `show_user_in_prompt = false`, F-Terminal hides the `user@host` prefix
from the visible prompt, including lines already printed on screen, and from
pane/window titles (for example `fiopans1@MacBook-Pro ~/repo %` becomes
`~/repo %`). It works on macOS, Linux and Windows, including when the pane is
narrow and the prompt wraps across rows; it preserves the colors of the rest of
the prompt and does not modify your `PS1`/`PROMPT` or shell history.

## Scrollback

- `scrollback_lines` is the per-pane limit for the active session. Default is
  **6000** lines. It is clamped to `500..100000`.
- `hidden_scrollback_lines` is the per-pane limit for sessions that are not
  active. Default is **3000** lines. It is clamped to
  `100..scrollback_lines`.
- "Hidden" means its tab/session is not the active one; collapsing a session in
  the sidebar does not reduce its history.
- When switching sessions, their panes rise to the active limit; when hidden
  they drop to the hidden limit and the oldest history beyond the new limit is
  discarded.
- `scrollback_budget_mb = 0` disables the global budget, which is the default.
  It is clamped to `0..4096`. When enabled, `columns × lines × 32 bytes` is
  estimated per pane, and history is trimmed first from hidden sessions, then
  from visible but unfocused panes, and finally from the focused pane. No pane
  drops below **1000** lines. If the budget is impossible to meet, the minimum
  is kept and a warning is logged.

For reference, a 120-column pane with 6000 lines is estimated at about 23 MB
of history.

## Shared or isolated rendering

- `mode = "shared"` (default): the main window creates a single
  `Instance`/`Adapter`/`Device`/`Queue` and a single font cache and glyph
  atlas. Code Review and the Git manager reuse those resources while keeping
  their own surface, viewport, scale factor and cell metrics. Atlases are
  split by texture format if the surfaces do not match.
- `mode = "isolated"`: each window creates its own renderer, device and text
  resources. This is the previous behavior and the conservative option if glyph
  or DPI issues appear.
- If shared rendering fails when opening a secondary window, a warning is
  logged and that window automatically falls back to an isolated renderer; the
  app keeps working.
- The mode is applied at startup. Changing it requires restarting the app.

## Restoring the previous behavior

To restore the previous scrollback and renderer, use:

```toml
[terminal]
scrollback_lines = 10000
hidden_scrollback_lines = 10000
scrollback_budget_mb = 0

[render]
mode = "isolated"
```

and restart F-Terminal.

## Shell profiles

Shell detection, the profile selector and the `[shell] default` field are
documented in [Shell profiles](shell-profiles.md).

---

[← Back to the index](README.md)
