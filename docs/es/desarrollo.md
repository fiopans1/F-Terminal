# Desarrollo

> [Documentación](../README.md) · [English](../en/development.md)

Esta página reúne el stack, la arquitectura, las pruebas, el estado del
proyecto y las vías de contribución.

## Stack

| Crate / herramienta | Uso |
|---|---|
| `winit` | Ventanas, teclado, IME y DPI |
| `wgpu` | GPU: Metal, Vulkan o DX12 |
| `glyphon` + `cosmic-text` | Shaping, texto y fallback de fuentes |
| `alacritty_terminal` | Estado VT: grid, scrollback, modos y selección |
| `vte` | Parser ANSI |
| `portable-pty` | PTY en Unix y ConPTY en Windows |
| `arboard` | Portapapeles |
| `muda` | Menú nativo de macOS |
| `sysinfo` | cwd del proceso hijo para seguir el `cd` |
| `syntect` | Resaltado con gramáticas TextMate |
| `toml` | Configuración legible y comentada |
| `serde_json` | Listas de PRs/MRs de GitHub y GitLab |
| `curl` (externo) | Peticiones a las APIs de GitHub y GitLab |

`curl` viene incluido en macOS, en la mayoría de distribuciones Linux y en
Windows 10/11. Solo se usa para la pestaña de PRs/MRs de Code Review.

## Arquitectura

```
src/
  main.rs          Arranque, config y event loop
  app.rs           Ventana principal, routing multi-ventana, atajos y estado
  agents.rs        Modelo de agentes, watcher de procesos y hooks de Junie
  agent_window.rs  Ventana Agent Mode
  complete/
    mod.rs         Motor, contexto, ranking y candidatos
    line.rs        Prompt, tokenizador y quoting
    history.rs     Historial compacto y parsers por shell
    commands.rs    Índice del PATH y builtins
    paths.rs       Rutas y caché del directorio actual
    git.rs         Refs Git directas para ramas/tags
  config.rs        Configuración TOML, validación, plantilla y recarga en caliente
  diff_view.rs     Render compartido de diffs
  editor.rs        Editor mínimo
  event.rs         Eventos de background hacia la UI
  explorer.rs      Árbol de archivos del cwd
  forge/
    mod.rs         Detección de GitHub/GitLab y modelo remoto común
    github.rs      API de pull requests de GitHub
    gitlab.rs      API de merge requests de GitLab
    http.rs        Runner curl compartido
  git/
    mod.rs         Snapshot y detección de repositorio
    command.rs     Runner seguro de git
    status.rs      Parser de git status --porcelain=v2 -z
    refs.rs        Ramas, remotos, tags y stashes
    history.rs     Log de commits
    ops.rs         Stage, commit, checkout, merge, stash, red, etc.
  git_status.rs    Rama y +N/-N para el footer de cada pane
  git_window.rs    Ventana del gestor Git
  highlight.rs     Resaltado TextMate en background
  input.rs         Encoder xterm de teclado y ratón
  layout.rs        Geometría de panes, splits, footer y divisores
  menu.rs          Menú nativo macOS
  pty.rs           PTY/ConPTY y hilo lector
  review.rs        Descubrimiento Git y parser de diffs
  review_window.rs Ventana de Code Review
  shell/
    mod.rs         TerminalProfile / ShellCatalog
    unix.rs        Detección en macOS/Linux
    windows.rs     cmd, PowerShell, Git Bash, MSYS2, Cygwin y WSL
  sidebar.rs       Árbol de sesiones/panes, selector de shells y hit-testing
  workspace.rs     Sesiones (tabs), panes, splits y PTYs
  term/
    mod.rs         Term + parser + selección + búsqueda y enlaces
    color.rs       Paleta y conversión sRGB
    frame.rs       Snapshot del grid
    program_status.rs  Parser OSC 7501 y notificaciones
  render/
    mod.rs         Superficie wgpu y recursos GPU/texto compartidos
    scene.rs       Panes, labels y quads
    quads.rs       Rectángulos/píldoras instanciados
    text.rs        Buffers, glyphon y atlas compartido
    quads.wgsl     Shader de quads con esquinas redondeadas
```

El empaquetado nativo vive en `scripts/`, los iconos en `assets/` y el
workflow de releases en `.github/workflows/installers.yml`.

### Flujo de datos

```
hilo PTY por pane ──bytes──> EventLoopProxy ──> vte ──> Term
                                                           │
teclado ──encoder xterm──> PTY del pane enfocado           ├─> Frame
ratón ──selección / scroll / SGR / divisores───────────────┘
                                                           │
                                          quads + glyphon ─┘──> GPU
```

Cada pane tiene su propio `Term` y su `Frame`. El workspace mantiene el árbol
de splits; la sidebar solo conoce IDs; el renderer solo recibe `Rect` y `Frame`.

### Multi-ventana

`App` mantiene:

- `WindowState`: terminal principal.
- `Option<ReviewWindow>`: Code Review.
- `Option<GitWindow>`: gestor Git.
- `Option<AgentWindow>`: Agent Mode.

Los eventos se enrutan por `WindowId`. Cada ventana tiene su propia superficie,
viewport, factor de escala y métricas de celda. Con `render.mode = "shared"`
las ventanas secundarias reutilizan el dispositivo GPU y el atlas de texto de
la principal; con `"isolated"` cada una crea su propio renderer. Si el modo
compartido falla, la ventana secundaria cae a aislado.

### Background y concurrencia

- Resaltado de sintaxis: worker con generación/revisión y debounce.
- Footer Git: un hilo por consulta de estado.
- Code Review: listas de PRs/MRs y diffs en hilos separados.
- Gestor Git: snapshots, detalles y operaciones en hilos separados.
- Agentes: watcher de procesos en un hilo; `OSC 7501` se parsea en el hilo de
  UI sin trabajo extra por frame.
- Los resultados llevan `request_id` para descartar respuestas obsoletas.
- Las operaciones mutantes de Git se serializan con un estado `busy`.
- Los tokens de GitHub y GitLab se pasan a `curl` por stdin.

## Tests

```bash
cargo test
```

Los tests de `render::text` necesitan un adaptador GPU real. En CI o en
entornos headless se omiten:

```bash
cargo test --all-targets -- --skip render::text::tests
```

La CI ejecuta formato, Clippy y tests en Linux, macOS y Windows. Los
instaladores completos y ConPTY se validan en el workflow de release de tags.

Cobertura actual:

- Parser VT → `Frame`: texto, colores, fondos y cursor.
- Geometría de splits, footer y divisores.
- Sidebar: árbol sesión → panes, plegado, Files, selector de perfiles y
  collapse/reveal.
- Explorador de archivos.
- Editor mínimo: edición, guardado, scroll y backspace.
- Resaltado de sintaxis.
- Parser de diffs de Code Review.
- Parser de `git status --porcelain=v2`.
- Parser de refs/stashes.
- Parser de historial.
- Integración real de Git en un repo temporal: stage, commit y rama.
- Detección de remotos GitHub/GitLab, incluidos grupos anidados.
- Redacción de tokens en la salida `Debug`.
- Parseo de PRs de GitHub y MRs de GitLab.
- Construcción de diffs unificados desde los cambios de GitLab.
- Config TOML: defaults, parseo, límites, plantilla y fallback de render.
- Política de scrollback: sesión activa/oculta, presupuesto global y mínimo.
- Directorio inicial al lanzar desde GUI o desde una terminal.
- Autocompletado: historial (zsh/bash/fish/PowerShell), prompts, tokenizador,
  rutas, refs Git, contexto de subcomandos, ranking y layout del popup.
- Mapeo de acciones del menú.
- Smoke test offscreen de GPU con dos panes, una etiqueta y un atlas de texto
  compartido entre dos ventanas.

Comprobaciones recomendadas antes de un cambio:

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

## Estado y limitaciones

Probado en macOS. Linux y Windows están soportados por el stack y compilan,
pero no se han validado en hardware real en este entorno.

Limitaciones conocidas:

- Sin pantalla de ajustes; `font_family` y `render.mode` requieren reiniciar la
  app, el resto de `config.toml` se recarga en caliente.
- Editor sin undo/redo, selección, file watching ni autoguardado. El resaltado
  de sintaxis se desactiva en archivos grandes (más de 64 KB, 1.000 líneas o
  líneas de más de 2 KB) para mantener la respuesta al teclear.
- Code Review: soporte de GitHub y GitLab; sin comentarios inline, submit de
  reviews ni resaltado de sintaxis dentro del diff.
- Self-hosted GitHub/GitLab se detecta en hosts `github.*` / `gitlab.*`; hosts
  con otros nombres necesitan configuración explícita (pendiente).
- Marcas de archivos revisados solo en memoria.
- Gestor Git sin rebase, cherry-pick, revert, tags, force push ni resolución
  integrada de conflictos.
- Pull solo fast-forward.
- Footer Git cuenta cambios tracked; los untracked no suman.
- Historial del gestor Git limitado a 300 commits.
- Sin menú nativo en Windows/Linux; se usan atajos y controles propios.
- Sin pestañas dentro de una misma ventana más allá del workspace.
- Shells de Windows implementados por detección y ConPTY, pero pendientes de
  prueba manual en Windows.
- En WSL, rutas UNC que no sean `\\wsl$` o `\\wsl.localhost` pueden requerir
  ajustes manuales.
- Sin emoji a color ni fallback avanzado por script.
- Sin render incremental por filas sucias ni límite de parseo por frame.
- Autocompletado: detección de prompt heurística; sin comandos multilínea ni
  completado en mitad de la línea; sin flags/plugins específicos por comando;
  los alias y funciones del shell solo aparecen si están en el historial.
- Autocompletado en WSL: no lee historiales ni refs internos de la distro; usa
  la sesión actual y el cwd visible.
- Autocompletado en fish: sin historial inline para no duplicar el nativo.

## Roadmap

- Validar perfiles de shell y ConPTY en Windows real.
- Pantalla de ajustes y perfiles de shell personalizados.
- Selector de tema en la app (las paletas ya están disponibles vía TOML).
- Añadir más perfiles: NuShell, fish en Windows, distros WSL concretas.
- Git: tags, rebase, cherry-pick, revert, force push con lease y resolución de
  conflictos.
- Code Review: Bitbucket/Azure DevOps, comentarios inline, submit de reviews y
  resaltado en el diff.
- Config por proveedor para hosts self-hosted con nombres arbitrarios.
- Aprobaciones y discusiones de MRs/PRs.
- Editor: undo/redo, selección, file watching y autoguardado.
- Sidebar: renombrar y actividad en segundo plano.
- Integración de shell (OSC 133) para completar comandos multilínea con
  precisión y añadir flags/plugins específicos por comando.
- Menú nativo en Windows/Linux.
- Mejoras de rendimiento: render incremental y menos re-shaping.
- Tests de integración de PTY y snapshots de frames.

## Contribuir

F-Terminal está en desarrollo activo y las contribuciones son bienvenidas:
bugs, mejoras, documentación, pruebas en Linux/Windows o nuevas ideas.

1. Lee [CONTRIBUTING.md](../../CONTRIBUTING.md).
2. Revisa el [roadmap](#roadmap) y los issues abiertos.
3. Abre un issue con la plantilla correspondiente antes de un cambio grande.
4. Ejecuta `cargo fmt`, `cargo clippy` y `cargo test` antes del pull request.

Al participar aceptas el [Código de Conducta](../../CODE_OF_CONDUCT.md). Las
contribuciones se publican bajo AGPL-3.0.

## Seguridad

No publiques vulnerabilidades en un issue público. Usa el reporte privado:

<https://github.com/fiopans1/F-Terminal/security/advisories/new>

Consulta [SECURITY.md](../../SECURITY.md) para versiones soportadas, alcance y
tiempos de respuesta.

## Changelog

Los cambios relevantes se documentan en [CHANGELOG.md](../../CHANGELOG.md).

## Licencia

F-Terminal se distribuye bajo la **GNU Affero General Public License v3.0**,
la misma licencia que
[`task-manager`](https://github.com/fiopans1/task-manager). Consulta
[`LICENSE`](../../LICENSE) para el texto completo.

Copyright © 2026 fiopans1.

---

[← Volver al índice](../README.md)
