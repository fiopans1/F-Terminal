# Características

> [Documentación](../README.md) · [English](../en/features.md)

F-Terminal es una terminal nativa en Rust para macOS, Linux y Windows, pensada
para el día a día sin Electron ni runtime web, manteniendo el binario y el
consumo de memoria bajos.

- Terminal real con PTY (Unix) y **ConPTY** (Windows), sin emulación de shell.
- Render GPU con `wgpu` y texto con `glyphon`/`cosmic-text`.
- Sidebar con árbol de sesiones y panes, más explorador de archivos.
- Identidad visual **Foundry** (grafito frío y latón cálido) y carcasa común en
  Git, Code Review y Agentes: cabecera, tipografía, foco y estados vacíos.
- Sesiones plegables con panes hijos y splits derecha/abajo; divisores
  arrastrables.
- Cada pane tiene su propio shell, cwd, scrollback y estado VT.
- Búsqueda literal en scrollback con *smart case*, navegación y resaltado de
  coincidencias; enlaces `OSC 8` y URLs planas con `Ctrl/Cmd+Click`.
- Sidebar con sesiones reordenables arrastrando y orden persistido.
- Barra de título propia en Windows y Linux, con icono, título, botón **?** de
  About y controles de minimizar/maximizar/cerrar; admite arrastre, doble clic
  para maximizar y redimensionado desde los bordes. En macOS se integra una
  única barra superior de altura nativa con los semáforos del sistema, el icono,
  **F-Terminal** y el botón **?**; Sessions queda como cabecera de la sidebar y
  la terminal empieza justo debajo de la barra.
- El panel About muestra el desarrollador y la versión, enlaza al repositorio y
  reserva una fila para el futuro botón de actualizaciones. En macOS también se
  abre desde el menú nativo de la aplicación.
- Selección con ratón, portapapeles, IME, bracketed paste y alternate screen.
- Colores ANSI de 16/256/truecolor, OSC 4/10/11 y consultas DA/DSR.
- Editor integrado con resaltado TextMate en background.
- **Code Review** en ventana separada: cambios locales, rama contra base,
  commits y **pull requests de GitHub o merge requests de GitLab** con buscador.
- **Gestor Git** en ventana separada: stage/unstage, commit/amend, ramas,
  fetch/pull/push, merge, stash e historial con diff.
- **Agent Mode** en ventana separada: centro de agentes con estados en tiempo
  real, telemetría CPU/RAM, diff Git, consola en vivo, subagentes, búsqueda y
  filtros, mensajes rápidos y lanzador de Claude Code, Codex, OpenCode y Junie.
- Footer Git por terminal con dos burbujas: rama y `+N/-N`.
- Autocompletado inline y popup: historial, comandos del `PATH`, rutas y
  ramas/tags de Git, sin indexar el disco ni lanzar `git` por pulsación.
- Perfiles de shell: zsh, bash, fish, `cmd.exe`, Windows PowerShell,
  PowerShell 7, Git Bash, MSYS2/Cygwin y distribuciones WSL.
- Configuración TOML con scrollback por sesión, presupuesto global opcional,
  modo de render compartido o aislado y recarga en caliente al guardar.
- Instaladores nativos offline: DMG en macOS, EXE con UAC en Windows y
  paquetes DEB/RPM en Linux.
- La terminal principal sigue usable mientras Code Review, el gestor Git o
  Agent Mode están abiertos.

## Rendimiento

- El render de terminal reutiliza el frame de cada pane y usa el daño por
  líneas de alacritty: una tecla o una línea nueva no obligan a reconstruir ni
  re-shapear toda la pantalla.
- El autocompletado se cachea por línea/cwd, no se ejecuta en redraws ajenos y
  ordena/trunca antes de materializar candidatos innecesarios.
- Las etiquetas de la interfaz y las medidas de texto se cachean; las ramas
  largas se truncan con búsqueda binaria.
- Git, Code Review, el explorador y el resaltado trabajan fuera del hilo UI.
  La lista local de Code Review y los snapshots de Git se cargan de forma
  asíncrona; los PR/MR se piden solo al abrir su pestaña.
- El polling de Git se agrupa por cwd, tiene backoff y no se solapa. El watcher
  de agentes evita pedir `cwd` para todos los procesos y comparte snapshots por
  `Arc`.
- Las ventanas minimizadas/ocluidas no preparan escenas que no se van a
  presentar.

---

[← Volver al índice](../README.md)
