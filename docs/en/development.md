# Development

> [Documentation](README.md) · [Español](../es/desarrollo.md)

This page collects the stack, architecture, tests, project status and ways to
contribute.

## Stack

| Crate / tool | Purpose |
|---|---|
| `winit` | Windows, keyboard, IME and DPI |
| `wgpu` | GPU: Metal, Vulkan or DX12 |
| `glyphon` + `cosmic-text` | Shaping, text and font fallback |
| `alacritty_terminal` | VT state: grid, scrollback, modes and selection |
| `vte` | ANSI parser |
| `portable-pty` | PTY on Unix and ConPTY on Windows |
| `arboard` | Clipboard |
| `muda` | Native macOS menu |
| `sysinfo` | Child process cwd to follow `cd` |
| `syntect` | Highlighting with TextMate grammars |
| `toml` | Readable, commented configuration |
| `serde_json` | GitHub and GitLab PR/MR lists |
| `curl` (external) | GitHub and GitLab API requests |

`curl` ships with macOS, most Linux distributions and Windows 10/11. It is only
used for the Code Review PR/MR tab.

## Architecture

```
src/
  main.rs          Startup, config and event loop
  app.rs           Main window, multi-window routing, shortcuts and state
  agents.rs        Agent model, process watcher and Junie hooks
  agent_window.rs  Agent Mode window
  complete/
    mod.rs         Engine, context, ranking and candidates
    line.rs        Prompt, tokenizer and quoting
    history.rs     Compact history and per-shell parsers
    commands.rs    PATH index and builtins
    paths.rs       Paths and current-directory cache
    git.rs         Direct Git refs for branches/tags
  config.rs        TOML configuration, validation, template and hot reload
  diff_view.rs     Shared diff rendering
  editor.rs        Minimal editor
  event.rs         Background events to the UI
  explorer.rs      cwd file tree
  forge/
    mod.rs         GitHub/GitLab detection and common remote model
    github.rs      GitHub pull request API
    gitlab.rs      GitLab merge request API
    http.rs        Shared curl runner
  git/
    mod.rs         Snapshot and repository detection
    command.rs     Safe git runner
    status.rs      git status --porcelain=v2 -z parser
    refs.rs        Branches, remotes, tags and stashes
    history.rs     Commit log
    ops.rs         Stage, commit, checkout, merge, stash, network, etc.
  git_status.rs    Branch and +N/-N for each pane's footer
  git_window.rs    Git manager window
  highlight.rs     Background TextMate highlighting
  input.rs         xterm keyboard and mouse encoder
  layout.rs        Pane geometry, splits, footer and dividers
  menu.rs          Native macOS menu
  pty.rs           PTY/ConPTY and reader thread
  review.rs        Git discovery and diff parser
  review_window.rs Code Review window
  shell/
    mod.rs         TerminalProfile / ShellCatalog
    unix.rs        macOS/Linux detection
    windows.rs     cmd, PowerShell, Git Bash, MSYS2, Cygwin and WSL
  sidebar.rs       Session/pane tree, shell selector and hit-testing
  workspace.rs     Sessions (tabs), panes, splits and PTYs
  term/
    mod.rs         Term + parser + selection + search and links
    color.rs       Palette and sRGB conversion
    frame.rs       Grid snapshot
    program_status.rs  OSC 7501 parser and notifications
  render/
    mod.rs         wgpu surface and shared GPU/text resources
    scene.rs       Panes, labels and quads
    quads.rs       Instanced rectangles/pills
    text.rs        Buffers, glyphon and shared atlas
    quads.wgsl     Quad shader with rounded corners
```

Native packaging lives in `scripts/`, icons in `assets/` and the release
workflow in `.github/workflows/installers.yml`.

### Data flow

```
PTY thread per pane ──bytes──> EventLoopProxy ──> vte ──> Term
                                                           │
keyboard ──xterm encoder──> focused pane's PTY             ├─> Frame
mouse ──selection / scroll / SGR / dividers────────────────┘
                                                           │
                                          quads + glyphon ─┘──> GPU
```

Each pane has its own `Term` and `Frame`. The workspace maintains the split
tree; the sidebar only knows IDs; the renderer only receives `Rect` and `Frame`.

### Multi-window

`App` holds:

- `WindowState`: main terminal.
- `Option<ReviewWindow>`: Code Review.
- `Option<GitWindow>`: Git manager.
- `Option<AgentWindow>`: Agent Mode.

Events are routed by `WindowId`. Each window has its own surface, viewport,
scale factor and cell metrics. With `render.mode = "shared"`, secondary windows
reuse the main window's GPU device and text atlas; with `"isolated"`, each one
creates its own renderer. If shared mode fails, the secondary window falls back
to isolated.

### Background and concurrency

- Syntax highlighting: worker with generation/revision and debounce.
- Git footer: one thread per status query.
- Code Review: PR/MR lists and diffs on separate threads.
- Git manager: snapshots, details and operations on separate threads.
- Agents: process watcher on a thread; `OSC 7501` is parsed on the UI thread
  with no extra per-frame work.
- Results carry a `request_id` to discard stale responses.
- Mutating Git operations are serialized with a `busy` state.
- GitHub and GitLab tokens are passed to `curl` through stdin.

## Tests

```bash
cargo test
```

The `render::text` tests need a real GPU adapter. In CI or headless
environments they are skipped:

```bash
cargo test --all-targets -- --skip render::text::tests
```

CI runs formatting, Clippy and tests on Linux, macOS and Windows. Full
installers and ConPTY are validated in the tag release workflow.

Current coverage:

- VT parser → `Frame`: text, colors, backgrounds and cursor.
- Split, footer and divider geometry.
- Sidebar: session → panes tree, collapsing, Files, profile selector and
  collapse/reveal.
- File explorer.
- Minimal editor: editing, saving, scrolling and backspace.
- Syntax highlighting.
- Code Review diff parser.
- `git status --porcelain=v2` parser.
- Refs/stashes parser.
- History parser.
- Real Git integration in a temporary repo: stage, commit and branch.
- GitHub/GitLab remote detection, including nested groups.
- Token redaction in `Debug` output.
- GitHub PR and GitLab MR parsing.
- Building unified diffs from GitLab changes.
- TOML config: defaults, parsing, limits, template and render fallback.
- Scrollback policy: active/hidden session, global budget and minimum.
- Initial directory when launched from GUI or from a terminal.
- Autocomplete: history (zsh/bash/fish/PowerShell), prompts, tokenizer, paths,
  Git refs, subcommand context, ranking and popup layout.
- Menu action mapping.
- Offscreen GPU smoke test with two panes, a label and a text atlas shared
  between two windows.

Recommended checks before a change:

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

## Status and limitations

Tested on macOS. Linux and Windows are supported by the stack and compile, but
they have not been validated on real hardware in this environment.

Known limitations:

- No settings screen; `font_family` and `render.mode` require restarting the
  app, the rest of `config.toml` reloads live.
- Editor without undo/redo, selection, file watching or autosave. Syntax
  highlighting is disabled for large files (over 64 KB, 1,000 lines or lines
  longer than 2 KB) to keep typing responsive.
- Code Review: GitHub and GitLab support; no inline comments, review submission
  or syntax highlighting inside the diff.
- Self-hosted GitHub/GitLab is detected on `github.*` / `gitlab.*` hosts; hosts
  with other names need explicit configuration (pending).
- Reviewed-file marks are in memory only.
- Git manager without rebase, cherry-pick, revert, tags, force push or built-in
  conflict resolution.
- Pull is fast-forward only.
- Git footer counts tracked changes; untracked files do not count.
- Git manager history is limited to 300 commits.
- No native menu on Windows/Linux; shortcuts and custom controls are used.
- No tabs within a single window beyond the workspace.
- Windows shells are implemented via detection and ConPTY, but still pending
  manual testing on Windows.
- Under WSL, UNC paths other than `\\wsl$` or `\\wsl.localhost` may require
  manual adjustments.
- No color emoji or advanced per-script fallback.
- No incremental rendering for dirty rows or per-frame parse limit.
- Autocomplete: heuristic prompt detection; no multiline commands or
  mid-line completion; no command-specific flags/plugins; shell aliases and
  functions only appear if they are in history.
- Autocomplete under WSL: does not read the distribution's histories or
  internal refs; uses the current session and the visible cwd.
- Autocomplete in fish: no inline history, to avoid duplicating the native one.

## Roadmap

- Validate shell profiles and ConPTY on real Windows.
- Settings screen and custom shell profiles.
- In-app theme selector (palettes are already available via TOML).
- Add more profiles: NuShell, fish on Windows, specific WSL distributions.
- Git: tags, rebase, cherry-pick, revert, force push with lease and conflict
  resolution.
- Code Review: Bitbucket/Azure DevOps, inline comments, review submission and
  diff highlighting.
- Per-provider config for self-hosted hosts with arbitrary names.
- MR/PR approvals and discussions.
- Editor: undo/redo, selection, file watching and autosave.
- Sidebar: renaming and background activity.
- Shell integration (OSC 133) to complete multiline commands accurately and add
  command-specific flags/plugins.
- Native menu on Windows/Linux.
- Performance improvements: incremental rendering and less reshaping.
- PTY integration tests and frame snapshots.

## Contributing

F-Terminal is under active development and contributions are welcome: bugs,
improvements, documentation, testing on Linux/Windows or new ideas.

1. Read [CONTRIBUTING.md](../../CONTRIBUTING.md).
2. Review the [roadmap](#roadmap) and open issues.
3. Open an issue with the appropriate template before a large change.
4. Run `cargo fmt`, `cargo clippy` and `cargo test` before the pull request.

By participating you agree to the [Code of Conduct](../../CODE_OF_CONDUCT.md).
Contributions are published under AGPL-3.0.

## Security

Do not publish vulnerabilities in a public issue. Use the private report:

<https://github.com/fiopans1/F-Terminal/security/advisories/new>

See [SECURITY.md](../../SECURITY.md) for supported versions, scope and
response times.

## Changelog

Relevant changes are documented in [CHANGELOG.md](../../CHANGELOG.md).

## License

F-Terminal is distributed under the **GNU Affero General Public License v3.0**,
the same license as
[`task-manager`](https://github.com/fiopans1/task-manager). See
[`LICENSE`](../../LICENSE) for the full text.

Copyright © 2026 fiopans1.

---

[← Back to the index](README.md)
