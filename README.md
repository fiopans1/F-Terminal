# F-Terminal

> Español · [English](README.en.md)

[![CI](https://github.com/fiopans1/F-Terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/fiopans1/F-Terminal/actions/workflows/ci.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg)](docs/es/instalacion.md#requisitos)
[![Status](https://img.shields.io/badge/status-early%20development-yellow.svg)](docs/es/desarrollo.md#estado-y-limitaciones)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

Terminal nativa en Rust, ligera y multiplataforma para **macOS, Linux y
Windows**. Usa un PTY real, parser VT, render GPU con fuentes del sistema y una
sidebar estilo Warp, sin Electron ni runtime web.

Incluye splits, explorador de archivos, editor con resaltado de sintaxis,
búsqueda en scrollback, autocompletado, footer Git por terminal y ventanas
independientes para **Code Review**, **Git** y **Agentes**.

> La documentación completa está en [`docs/`](docs/README.md), en español e
> inglés. También hay una versión en inglés del README en
> [`README.en.md`](README.en.md).

## Características

- PTY real en Unix y ConPTY en Windows, con soporte VT completo.
- Render GPU con `wgpu`, `glyphon` y `cosmic-text`.
- Sidebar con sesiones plegables, panes, splits y explorador de archivos.
- Búsqueda literal en scrollback y enlaces con `Ctrl/Cmd+Click`.
- Editor integrado con resaltado TextMate.
- Autocompletado de historial, comandos, rutas y ramas/tags de Git.
- **Code Review**: cambios locales, commits y PRs/MRs de GitHub y GitLab.
- **Gestor Git**: stage, commits, ramas, remotos, stash e historial.
- **Agent Mode**: supervisión y control de Claude Code, Codex, OpenCode y Junie.
- Perfiles de shell en macOS, Linux y Windows (incluido WSL).
- Instaladores nativos offline para macOS, Linux y Windows.

Lista completa y notas de rendimiento en
[Características](docs/es/caracteristicas.md).

## Inicio rápido

Requisitos: Rust 1.95+, Git, un backend gráfico y `curl` para PRs/MRs.

```bash
cargo run
```

Para compilar en release, generar instaladores o ver los requisitos completos,
consulta [Instalación](docs/es/instalacion.md).

## Documentación

| Tema | Página |
|---|---|
| Características y rendimiento | [docs/es/caracteristicas.md](docs/es/caracteristicas.md) |
| Instalación y compilación | [docs/es/instalacion.md](docs/es/instalacion.md) |
| Perfiles de shell | [docs/es/perfiles-de-shell.md](docs/es/perfiles-de-shell.md) |
| Configuración | [docs/es/configuracion.md](docs/es/configuracion.md) |
| Terminal y búsqueda | [docs/es/terminal.md](docs/es/terminal.md) |
| Autocompletado | [docs/es/autocompletado.md](docs/es/autocompletado.md) |
| Sidebar, editor y footer Git | [docs/es/interfaz.md](docs/es/interfaz.md) |
| Code Review | [docs/es/code-review.md](docs/es/code-review.md) |
| Gestor Git | [docs/es/gestor-git.md](docs/es/gestor-git.md) |
| Agent Mode | [docs/es/agent-mode.md](docs/es/agent-mode.md) |
| Atajos | [docs/es/atajos.md](docs/es/atajos.md) |
| Autenticación | [docs/es/autenticacion.md](docs/es/autenticacion.md) |
| Arquitectura y desarrollo | [docs/es/desarrollo.md](docs/es/desarrollo.md) |

## Contribuir

Las contribuciones son bienvenidas. Antes de abrir un pull request, lee
[CONTRIBUTING.md](CONTRIBUTING.md), revisa el
[roadmap](docs/es/desarrollo.md#roadmap) y ejecuta `cargo fmt`,
`cargo clippy` y `cargo test`.

Para reportar vulnerabilidades, usa el
[reporte privado](https://github.com/fiopans1/F-Terminal/security/advisories/new)
y consulta [SECURITY.md](SECURITY.md).

## Licencia

F-Terminal se distribuye bajo la **GNU Affero General Public License v3.0**.
Consulta [LICENSE](LICENSE) para el texto completo.

Copyright © 2026 fiopans1.
