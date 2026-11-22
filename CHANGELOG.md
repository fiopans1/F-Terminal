# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí. El formato
sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto
usa [Versionado Semántico](https://semver.org/lang/es/).

## Unreleased

### Added

- Autocompletado inline y popup: historial, comandos del `PATH`, subcomandos de
  Git, rutas y ramas/tags, con cachés acotadas y sin procesos por pulsación.
- CI de formato, Clippy y tests en Linux y macOS.
- Plantillas de issues y pull requests, guía de contribución, código de
  conducta, política de seguridad y Dependabot.

### Changed

- Los tests de render GPU se omiten en CI porque requieren un adaptador real.

## 0.1.0 - 2026-11-22

### Added

- Terminal nativa con PTY/ConPTY, parser VT, scrollback y render GPU.
- Sidebar de sesiones y panes, splits y explorador de archivos.
- Editor integrado con resaltado TextMate.
- Ventana de Code Review para cambios locales, commits y PRs/MRs de GitHub y
  GitLab.
- Gestor Git con stage, commit, ramas, remoto, stash e historial.
- Footer Git por terminal y perfiles de shell para macOS, Linux y Windows.
- Configuración TOML, perfiles de shell, instaladores DMG/EXE/DEB/RPM y menú
  nativo de macOS.
