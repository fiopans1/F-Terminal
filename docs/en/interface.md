# Sidebar, sessions, explorer and editor

> [Documentation](README.md) · [Español](../es/interfaz.md)

## Sidebar, sessions and splits

The sidebar has two modes:

- **Sessions**: tree of sessions/workspaces and their panes.
- **Files**: explorer for the focused pane's cwd.

### Session tree

Each session is a workspace with one or more panes:

- A session with a single pane occupies one row.
- When the session has multiple panes, a parent row appears with the panes as
  children, using `├`/`└` connectors.
- The parent shows a `▾`/`▸` chevron to collapse or expand its children and a
  pane counter.
- A session's initial title is **New session**. When the shell sends its title
  via OSC, the parent is updated.
- Titles that are only a path (for example, the current directory reported by
  ConPTY/cmd.exe on Windows) are ignored so the sidebar stays clean.
- Children show the pane title or, if there is none, the shell (`zsh`, `bash`,
  `PowerShell`…).
- Clicking the parent: focuses that session's active pane.
- Clicking a child: focuses that specific pane.
- `×` on the parent closes the entire session.
- `×` on a child closes only that pane.
- The focused pane is highlighted with an accent bar.
- Drag a session from its row to reorder it; an accent line marks the target
  position and the order is saved with the session.
- Right-clicking a session opens a menu with **Pin tab**/**Unpin tab** and
  **Rename**. Pinned sessions are grouped at the top and cannot be dragged out
  of that group; the state is saved.
- **Rename** (or `F2` on the active session, or double-clicking the row) edits
  the title inline: `Enter` confirms, `Esc` cancels and leaving it empty
  restores the automatic title. The custom name is saved and takes precedence
  over the shell-reported title.

Actions:

- `+` creates a new session with the default shell.
- `▾` opens the profile selector; it only appears in Sessions.
- The **E/T** button in the header toggles Files/Sessions; globally it uses
  `Ctrl+Shift+E` / `Cmd+Shift+E`.
- `«` collapses the sidebar; the `»` rail restores it.
- Mouse wheel to scroll if there are many sessions or panes.

Inside each session:

- `Cmd+D` / `Ctrl+Shift+D`: split right.
- `Cmd+Shift+S` / `Ctrl+Shift+S`: split down.
- New panes appear as children in the tree and expand the session.
- Dividers are dragged with the mouse.
- The focused pane shows an accent border.
- `Cmd+Alt+arrows` / `Ctrl+Alt+arrows`: move focus.
- <kbd>Cmd+&#96;</kbd> / <kbd>Ctrl+Shift+&#96;</kbd>: cycle panes.

### Session restore

When F-Terminal closes, the main window state is saved automatically and
restored on the next launch:

- Open tabs, their order and collapsed tabs.
- Splits with their direction and ratio, and the focused pane.
- Each pane's shell profile and working directory.
- Window size, position and maximized state.
- Each terminal's scrollback (text and colors, without OSC sequences).

Processes that were running are not restored: new shells are launched in the
saved directory. Command history still lives in the shell.

Saving is automatic and does not block the UI: metadata is written at most
every 2 seconds, and scrollback is exported in a sweep spread across frames
(one pane per tick) every 30 seconds, plus a final save on close. Serialization
and writing happen on a separate thread.

Restoration does not block either: the active tab's panes are pre-started with
the last saved grid while the GPU and fonts initialize, and the rest are
created in the background when their tab is opened. The window shows a
placeholder until each shell is ready; keystrokes made meanwhile are preserved.
The font index is cached in the system cache directory so warm starts do not
have to scan every installed font.

The data lives alongside the configuration:

| Platform | Path |
|---|---|
| macOS / Linux | `~/.config/f-terminal/session.json` |
| Windows | `%APPDATA%\f-terminal\session.json` |

Scrollback is saved in `session-scrollback/*.ansi` and may contain sensitive
information that appeared in the terminal.

To disable it, in `config.toml`:

```toml
[session]
restore = false
```

If you want to restore tabs and directories but not the previous content, keep
`restore = true` and set `restore_scrollback = false`.

## Explorer and editor

### Explorer

- Follows the focused shell's real cwd.
- Folders expandable with `▸`/`▾`.
- Files open with a single click.
- `←` button to go up to the parent directory. Manual navigation is kept until
  the shell's cwd or the focused pane changes.
- `↻` button to refresh.
- It does not use file watching: it updates on `cd` or on refresh.

### Editor

- Panel to the right of the terminal.
- Draggable divider for resizing.
- `Cmd+S` / `Ctrl+S` saves.
- `Esc` or `Cmd+W` closes. The accent dot indicates unsaved changes.
- Clicking with the mouse places the cursor at the clicked row and column.
- TextMate highlighting with `syntect` on a separate thread, with a 400 ms
  debounce and a single in-flight job so complete files are not queued while
  you type.
- `.jsx`, `.mjs`, `.cjs`, `.ts` and `.tsx` use the JavaScript grammar as a
  fallback; `.jsonc` and `.json5` use the JSON grammar. TOML does not yet have
  a grammar in the default package and is displayed without colors.
- Text buffers are cached per pane; on edit only the edited row is reshaped,
  and cursor movement does not reshape text.

Current editor limitations:

- No undo/redo or text selection.
- No file watching or autosave.
- No live highlighting for large files: over 64 KB, over 1,000 lines or lines
  longer than 2 KB.
- UTF-8 only, files up to 2 MB.
- Tabs are expanded to 4 spaces on load.
- Closing the editor discards unsaved changes.

## Per-terminal Git footer

Each pane shows two bubbles at the bottom left in a compact single-line bar:

1. Current branch, with an accent dot.
2. Working tree changes: `+N -N`, in a single pill with the green half for
   additions and the red half for deletions.

Details:

- The bubbles have a vector icon (branch and diff file), a border and a small
  bottom margin.
- Clicking the branch bubble: opens a popup with local branches. You can use
  `↑↓`, `Home`/`End` and `Enter`, or click a branch to check it out. `Esc` or a
  click outside closes it.
- Clicking the changes bubble: opens a split view on the right with the full
  working tree diff, with per-file headers, line numbers and the
  addition/deletion colors used by Code Review. `R` reloads, `Esc` or `×`
  closes it, the wheel/`↑↓`/`PgUp`/`PgDn` scroll and `Shift`+wheel or
  `←`/`→` scroll horizontally.
- The terminal panel is painted with the TUI's actual background (including
  opencode's SGR colors), so the footer and leftover space do not show a strip
  of the app color.
- The footer has a compact height and uses the grid's leftover space to keep
  the typography legible.
- It uses `git diff --numstat HEAD`, so it counts **tracked** staged and
  unstaged changes. Untracked files do not count.
- It updates when the cwd changes and every 2 seconds.
- Git inspection runs on a background thread per pane.
- Bubble width is measured with `cosmic-text`, not estimated per character.
- Outside a Git repository, the bubbles are not drawn.
- Clicks and wheel events inside the footer area are not sent to the PTY.

---

[← Back to the index](README.md)
