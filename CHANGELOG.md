# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí. El formato
sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto
usa [Versionado Semántico](https://semver.org/lang/es/).

## Unreleased

### Added

- Autocompletado inline y popup: historial, comandos del `PATH`, subcomandos de
  Git, rutas y ramas/tags, con cachés acotadas y sin procesos por pulsación.
- Agent Mode: ventana independiente con agentes detectados dentro y fuera de
  F-Terminal, estados en tiempo real vía `OSC 7501`, watcher de procesos y
  soporte opcional de hooks de Junie.
- Agent Mode permite forzar un escaneo inmediato con `R`/`F5` o el botón
  **REFRESH**, y terminar el proceso seleccionado con `X`/`Supr` o **KILL PID**
  (confirmación en dos pasos; `SIGTERM` y después `SIGKILL` si es necesario).
- Nueva sección `[agents]` en `config.toml` (`enabled`, `show_external`,
  `poll_interval_ms`).
- `Cmd+Shift+S` / `Ctrl+Shift+S` abre `config.toml` en el editor integrado; el
  elemento de menú «Abrir configuración…» hace lo mismo.
- Temas: Catppuccin Mocha, Dracula, Nord, Gruvbox Dark, One Dark, Solarized
  Dark, GitHub Dark y Kanagawa, además de Tokyo Night.
- `padding_left` añade un margen izquierdo configurable al texto del terminal
  sin recuperar el marco en los demás lados.
- El editor resalta `.jsx`, `.mjs`, `.cjs`, `.ts` y `.tsx` con la gramática de
  JavaScript, y `.jsonc`/`.json5` con la de JSON.
- `show_user_in_prompt = false` oculta el prefijo `user@host` del prompt y de
  los títulos en macOS, Linux y Windows sin tocar la configuración del shell.
- El footer Git por terminal permite abrir un popup de ramas locales y hacer
  checkout, y abrir los diffs completos (`git diff HEAD`) en una vista dividida
  formateada como Code Review.
- La terminal ocupa el panel de borde a borde (sin marco) y usa el fondo real
  del TUI, incluidos los colores SGR de opencode, para que no aparezca una
  franja del color de la app alrededor del footer ni del grid. El footer Git es
  compacto y aprovecha el espacio sobrante del grid. Los splits ya no dibujan un
  borde de foco alrededor del pane; el foco sigue marcado en la sidebar.
- CI de formato, Clippy y tests en Linux y macOS.
- Plantillas de issues y pull requests, guía de contribución, código de
  conducta, política de seguridad y Dependabot.

### Changed

- GitHub Dark es ahora el tema por defecto; la plantilla documenta todas las
  paletas disponibles.
- El gestor Git usa `Cmd+Shift+M` / `Ctrl+Shift+M`; `Ctrl+Shift+G` queda libre
  porque Windows lo intercepta para la grabación de la Xbox Game Bar.
- Las burbujas de rama y de `+N/-N` del footer son más grandes, con borde,
  iconos vectoriales, más separación y margen inferior.
- Los botones de cerrar de la sidebar y del panel lateral se dibujan como icono
  vectorial, no como glifo de fuente, para que se vean igual en Windows.
- Los tests de render GPU se omiten en CI porque requieren un adaptador real.

### Fixed

- Los títulos de ventana que solo contienen una ruta (ConPTY/cmd.exe en
  Windows) se ignoran; la sesión vuelve a mostrar **New session** en vez del
  directorio actual.
- El watcher de procesos de Agent Mode se reinicia si no responde en 8 segundos
  o si su hilo terminó, en lugar de quedarse sin actualizar la lista.

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
