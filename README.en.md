# F-Terminal

> English · [Español](README.md)

[![CI](https://github.com/fiopans1/F-Terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/fiopans1/F-Terminal/actions/workflows/ci.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg)](docs/en/installation.md#requirements)
[![Status](https://img.shields.io/badge/status-early%20development-yellow.svg)](docs/en/development.md#status-and-limitations)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

A lightweight, cross-platform native terminal written in Rust for **macOS,
Linux and Windows**. It uses a real PTY, a VT parser, GPU rendering with system
fonts and a Warp-style sidebar, with no Electron or web runtime.

It includes splits, a file explorer, a syntax-highlighted editor, scrollback
search, autocomplete, a per-terminal Git footer and separate windows for
**Code Review**, **Git** and **Agents**.

> Full documentation lives in [`docs/`](docs/en/README.md), in English and
> Spanish. The Spanish README is available at [`README.md`](README.md).

## Features

- Real PTY on Unix and ConPTY on Windows, with full VT support.
- GPU rendering with `wgpu`, `glyphon` and `cosmic-text`.
- Sidebar with collapsible sessions, panes, splits and a file explorer.
- Literal scrollback search and links with `Ctrl/Cmd+Click`.
- Built-in editor with TextMate highlighting.
- Autocomplete for history, commands, paths and Git branches/tags.
- **Code Review**: local changes, commits and GitHub/GitLab PRs/MRs.
- **Git manager**: staging, commits, branches, remotes, stash and history.
- **Agent Mode**: supervision and control of Claude Code, Codex, OpenCode and
  Junie.
- Shell profiles on macOS, Linux and Windows (including WSL).
- Offline native installers for macOS, Linux and Windows.

See the full list and performance notes in
[Features](docs/en/features.md).

## Quick start

Requirements: Rust 1.95+, Git, a graphics backend and `curl` for PRs/MRs.

```bash
cargo run
```

For release builds, installer generation or the full requirements, see
[Installation](docs/en/installation.md).

## Documentation

| Topic | Page |
|---|---|
| Features and performance | [docs/en/features.md](docs/en/features.md) |
| Installation and build | [docs/en/installation.md](docs/en/installation.md) |
| Shell profiles | [docs/en/shell-profiles.md](docs/en/shell-profiles.md) |
| Configuration | [docs/en/configuration.md](docs/en/configuration.md) |
| Terminal and search | [docs/en/terminal.md](docs/en/terminal.md) |
| Autocomplete | [docs/en/autocomplete.md](docs/en/autocomplete.md) |
| Sidebar, editor and Git footer | [docs/en/interface.md](docs/en/interface.md) |
| Code Review | [docs/en/code-review.md](docs/en/code-review.md) |
| Git manager | [docs/en/git-manager.md](docs/en/git-manager.md) |
| Agent Mode | [docs/en/agent-mode.md](docs/en/agent-mode.md) |
| Shortcuts | [docs/en/shortcuts.md](docs/en/shortcuts.md) |
| Authentication | [docs/en/authentication.md](docs/en/authentication.md) |
| Architecture and development | [docs/en/development.md](docs/en/development.md) |

## Contributing

Contributions are welcome. Before opening a pull request, read
[CONTRIBUTING.md](CONTRIBUTING.md), review the
[roadmap](docs/en/development.md#roadmap) and run `cargo fmt`, `cargo clippy`
and `cargo test`.

To report vulnerabilities, use the
[private report](https://github.com/fiopans1/F-Terminal/security/advisories/new)
and see [SECURITY.md](SECURITY.md).

## License

F-Terminal is distributed under the **GNU Affero General Public License v3.0**.
See [LICENSE](LICENSE) for the full text.

Copyright © 2026 fiopans1.
