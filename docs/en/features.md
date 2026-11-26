# Features

> [Documentation](README.md) · [Español](../es/caracteristicas.md)

F-Terminal is a native terminal written in Rust for macOS, Linux and Windows,
designed for everyday use without Electron or a web runtime, keeping the binary
and memory usage low.

- Real terminal with PTY (Unix) and **ConPTY** (Windows), without shell
  emulation.
- GPU rendering with `wgpu` and text with `glyphon`/`cosmic-text`.
- Sidebar with a session and pane tree, plus a file explorer.
- **Foundry** visual identity (cool graphite and warm brass) and a shared shell
  across Git, Code Review and Agents: header, typography, focus and empty
  states.
- Collapsible sessions with child panes and right/down splits; draggable
  dividers.
- Each pane has its own shell, cwd, scrollback and VT state.
- Literal scrollback search with *smart case*, navigation and match
  highlighting; `OSC 8` links and plain URLs with `Ctrl/Cmd+Click`.
- Sidebar with drag-to-reorder sessions and persisted order.
- Custom title bar on Windows and Linux, with icon, title, **?** About button
  and minimize/maximize/close controls; it supports dragging, double-click to
  maximize and edge resizing. On macOS, a single native-height top bar
  integrates the system traffic lights, the icon, **F-Terminal** and the **?**
  button; Sessions becomes the sidebar header and the terminal starts right
  below the bar.
- The About panel shows the developer and version, links to the repository and
  reserves a row for the future updates button. On macOS it also opens from the
  native application menu.
- Mouse selection, clipboard, IME, bracketed paste and alternate screen.
- 16/256/truecolor ANSI colors, OSC 4/10/11 and DA/DSR queries.
- Built-in editor with background TextMate highlighting.
- **Code Review** in a separate window: local changes, branch against base,
  commits and **GitHub pull requests or GitLab merge requests** with search.
- **Git manager** in a separate window: stage/unstage, commit/amend, branches,
  fetch/pull/push, merge, stash and history with diff.
- **Agent Mode** in a separate window: agent center with real-time statuses,
  CPU/RAM telemetry, Git diff, live console, subagents, search and filters,
  quick messages and a launcher for Claude Code, Codex, OpenCode and Junie.
- Per-terminal Git footer with two bubbles: branch and `+N/-N`.
- Inline and popup autocomplete: history, `PATH` commands, paths and Git
  branches/tags, without indexing the disk or launching `git` per keystroke.
- Shell profiles: zsh, bash, fish, `cmd.exe`, Windows PowerShell, PowerShell 7,
  Git Bash, MSYS2/Cygwin and WSL distributions.
- TOML configuration with per-session scrollback, optional global budget,
  shared or isolated rendering and hot reload on save.
- Offline native installers: DMG on macOS, EXE with UAC on Windows and DEB/RPM
  packages on Linux.
- The main terminal remains usable while Code Review, the Git manager or Agent
  Mode are open.

## Performance

- Terminal rendering reuses each pane's frame and uses alacritty's per-line
  damage tracking: a keystroke or a new line does not force rebuilding or
  reshaping the whole screen.
- Autocomplete is cached per line/cwd, does not run on unrelated redraws, and
  sorts/truncates before materializing unnecessary candidates.
- UI labels and text measurements are cached; long branches are truncated with
  binary search.
- Git, Code Review, the explorer and highlighting work off the UI thread. The
  local Code Review list and Git snapshots load asynchronously; PRs/MRs are
  requested only when their tab is opened.
- Git polling is grouped by cwd, has backoff and does not overlap. The agent
  watcher avoids requesting `cwd` for every process and shares snapshots via
  `Arc`.
- Minimized/occluded windows do not prepare scenes that will not be presented.

---

[← Back to the index](README.md)
