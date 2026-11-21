# F-Terminal

Terminal nativa en Rust, ligera y multiplataforma (macOS, Linux y Windows).
PTY real, parser VT, render GPU con fuentes del sistema y una sidebar estilo
Warp para tener varias terminales abiertas, con splits dentro de cada una.

## Stack

| Crate | Uso |
|---|---|
| `winit` | Ventana, teclado, IME, DPI |
| `wgpu` | GPU: Metal / DX12 / Vulkan |
| `glyphon` + `cosmic-text` | Texto, shaping y fallback de fuentes |
| `alacritty_terminal` | Estado VT: grid, scrollback, modos, selección |
| `vte` | Parser ANSI |
| `portable-pty` | PTY en Unix y ConPTY en Windows |
| `arboard` | Portapapeles |
| `muda` | Menú nativo de macOS (solo esa plataforma) |
| `sysinfo` | cwd del proceso hijo para seguir el `cd` del shell |
| `syntect` | Resaltado de sintaxis con gramáticas TextMate |

## Ejecutar

```bash
cargo run
```

Con logs de depuración:

```bash
RUST_LOG=debug cargo run
```

Shell: usa `$SHELL` en Unix y `%COMSPEC%` en Windows. En macOS se lanza como
login shell (`-l`) para heredar el entorno del usuario.

## Arquitectura

```
src/
  main.rs          Arranque del event loop
  app.rs           Ventana, eventos, atajos, selección
  config.rs        Configuración (por ahora defaults en código)
  editor.rs        Editor de texto mínimo (editar, guardar, cursor)
  event.rs         Eventos internos PTY -> UI (etiquetados por pane)
  explorer.rs      Árbol de archivos del cwd del pane enfocado
  git_status.rs    Rama y estadísticas del working tree por pane
  highlight.rs     Resaltado TextMate con syntect
  input.rs         Encoder xterm de teclado y ratón
  layout.rs        Geometría de panes, splits y divisores
  menu.rs          Menú nativo macOS + acciones de explorador/VS Code
  pty.rs           Sesión PTY + hilo lector
  review.rs        Code Review: Git, parser de diffs y estado
  review_window.rs Ventana independiente de Code Review
  sidebar.rs       Layout y hit-testing de la sidebar
  workspace.rs     Tabs, panes, árbol de splits y sesiones PTY
  term/
    mod.rs         Wrapper de Term + parser + selección
    color.rs       Paleta, resolución de colores, sRGB -> linear
    frame.rs       Snapshot inmutable del grid listo para render
  render/
    mod.rs         Superficie wgpu y orquestación del frame
    scene.rs       Escena: panes + etiquetas + quads
    quads.rs       Rectángulos instanciados (fondos, cursor, chrome)
    text.rs        Buffers de texto + glyphon
    quads.wgsl     Shader de los quads
```

Flujo de datos:

```
hilo PTY por pane ──bytes──> EventLoopProxy ──> vte::Processor ──> Term
                                                                   │
teclado ──encoder xterm──> PTY del pane enfocado                  ├─> Frame
ratón ──selección / scroll / SGR / divisores───────────────────────┘
                                                                   │
                                             quads + glyphon ──────┘──> GPU
```

Cada pane tiene su propio PTY, su `Term` y su `Frame`. El workspace mantiene
un árbol binario de splits por terminal (sidebar item). La sidebar solo conoce
IDs de tab; el renderer solo conoce `Rect` + `Frame`.

El renderer no conoce `alacritty_terminal`: consume un `Frame` ya resuelto
(texto, colores, fondos, cursor). Eso mantiene aislado el motor VT de la capa
gráfica.

## Explorador y editor

- Botón `F` en la cabecera de la sidebar → modo **FILES**; `T` vuelve a terminales.
- El explorador muestra el cwd real del pane enfocado. Se actualiza solo cuando
  el shell hace `cd` (polling del proceso hijo cada 750 ms con `sysinfo`).
- Carpetas expandibles con `▸`/`▾`, archivos abribles con un clic y refresh
  manual con `↻`.
- Al abrir un archivo se abre un panel de editor a la derecha; las terminales
  se reajustan.
- `Cmd+S` / `Ctrl+S` guarda; `Esc` o `Cmd+W` cierra el editor.
- El divisor entre terminal y editor se arrastra con el ratón.
- Limitaciones actuales: sin undo, selección, resaltado en vivo para archivos
  grandes (>512 KB), file watching ni autoguardado; cerrar el editor descarta
  los cambios no guardados. Solo UTF-8 y archivos de hasta 2 MB. Los tabs se
  expanden a 4 espacios al cargar.

## Code Review

- `Cmd+Shift+R` / `Ctrl+Shift+R` o el ítem **Code Review** del menú de macOS.
- Abre una **ventana nativa separada**; la terminal principal sigue funcionando.
- Toma el repositorio Git del cwd del pane enfocado.
- Tres columnas (reviews, selector de archivos y diff) y dos pestañas:
  - **LOCAL**: working changes, rama contra `main`/`master` y los últimos 15
    commits.
  - **PULL REQUESTS**: todas las PRs abiertas del remoto de GitHub, con buscador.
- En **LOCAL**, `↑↓`/`Enter` o clic abren una review y `R` refresca.
- En **PULL REQUESTS**, `/` o `Cmd+F`/`Ctrl+F` enfocan el buscador; filtra por
  número, título, autor o rama (varios términos separados por espacios).
  `↑↓` mueve, `Enter` carga el diff y `R` recarga la lista. `Esc` sale del
  buscador sin cerrar la ventana.
- En el selector de archivos: `↑↓` mueve, `Espacio`/`M` marca el archivo como
  revisado (solo durante la sesión), `Enter` pasa al diff.
- En el diff: `↑↓`/PgUp/PgDn desplazan, `←→` horizontal.
- `Tab` cambia de panel; `Esc` o `×` cierran la ventana.
- La lista de PRs y el diff se cargan en hilos aparte.
- Autenticación de GitHub: los repos públicos funcionan sin token (con el límite
  de peticiones de la API). Para repos privados, en orden de prioridad:
  1. `GITHUB_TOKEN` o `GH_TOKEN` en el entorno.
  2. `~/.config/f-terminal/github-token` (o `%APPDATA%\f-terminal\github-token`
     en Windows), con el token en la primera línea.
  3. `gh auth token`, si `gh` está instalado y autenticado.
  El token necesita permiso de lectura sobre *Pull requests* (y *Contents* si
  usas un token fine-grained).
- Limitaciones: solo GitHub local (no GitLab), sin comentarios inline, sin
  revisión por IA, sin resaltado de sintaxis dentro del diff, corte a 50.000
  líneas y las marcas de revisado no se persisten.

## Rama y cambios por terminal

- Cada pane muestra dos burbujas agrupadas abajo a la izquierda: una con la
  rama actual y otra con los cambios `+N -N` (añadidas/eliminadas).
- Las estadísticas salen de `git diff --numstat HEAD` (cambios tracked, staged
  y unstaged) y se actualizan al cambiar el cwd y cada 2 segundos.
- La inspección corre en un hilo de background por pane, así que un repo grande
  no bloquea la terminal.
- Si el pane no está dentro de un repositorio Git, no se dibujan las burbujas.

## Incluido en esta iteración

- Ventana nativa con superficie wgpu (Metal en macOS).
- Sidebar lateral estilo Warp con la lista de terminales: crear (`+`), cambiar
  y cerrar (`×`), con scroll si hay muchas.
- Segundo modo de sidebar **FILES**: explorador del directorio actual del pane
  enfocado, con carpetas expandibles y refresco manual.
- Editor integrado con resaltado de sintaxis TextMate (syntect, la misma
  familia de gramáticas que usa VS Code). El resaltado corre en un hilo aparte
  con debounce y comprobación de revisión, así que escribir no bloquea la UI.
- El editor ocupa un panel a la derecha y su divisor se puede arrastrar para
  redimensionarlo, igual que los splits de terminales.
- El explorador y el menú **Abrir explorador / Abrir en VS Code** usan el cwd
  real del shell enfocado (se actualiza al hacer `cd`).
- Las terminales nuevas y los splits heredan el cwd del pane enfocado.
- Colapsar la sidebar con `«` y recuperarla con el rail `»`; también con atajo.
- Menú nativo en macOS: Abrir explorador, Abrir en VS Code, Code Review,
  Ajustes (placeholder) y Salir.
- Ventana **Code Review** independiente sobre el repositorio del pane enfocado:
  cambios locales, rama contra `main`/`master`, últimos commits y todas las
  **pull requests abiertas** de GitHub con buscador. La terminal sigue usable
  mientras la review está abierta.
- Selector de archivos al lado del diff, con estado (M/A/D/R), líneas añadidas
  y eliminadas, y marca de revisado por archivo durante la sesión.
- Al seleccionar una review, el diff por archivo se carga en background con
  numeración, altas/bajas coloreadas y navegación por teclado o ratón.
- Footer de Git en cada terminal con dos burbujas: rama y líneas
  añadidas/eliminadas del working tree, actualizado en background.
- Varias terminales abiertas a la vez, cada una con su propio shell y estado.
- Splits dentro de una terminal: derecha/abajo, con foco por clic, borde de
  foco y divisores arrastrables.
- Shell real en PTY con `TERM=xterm-256color` y `COLORTERM=truecolor`.
- Render multi-pane con una fuente monoespaciada del sistema y DPI correcto.
- Colores ANSI de 16/256 y truecolor, atributos inverse/hidden, selección.
- Alternate screen, scrollback por pane (rueda y PageUp/PageDown).
- Selección con ratón, doble clic para seleccionar palabra, copiar/pegar.
- Respuestas DA/DSR y consultas de color, necesarias para TUIs como opencode.
- Título de ventana vía OSC, foco, IME commit, bracketed paste.
- Resize de ventana -> resize real de cada PTY (`TIOCSWINSZ`/ConPTY).
- Sidebar ocultable con `Cmd+B` / `Ctrl+Shift+B`.

## Atajos

macOS:

| Acción | Atajo |
|---|---|
| Nueva terminal | `Cmd+T` |
| Cerrar pane / terminal | `Cmd+W` / `Cmd+Shift+W` |
| Split derecha / abajo | `Cmd+D` / `Cmd+Shift+D` |
| Cambiar de terminal | `Cmd+Shift+[` / `]`, `Cmd+1..9`, `Ctrl+Tab` |
| Mover foco entre panes | `Cmd+Alt+flechas`, `Cmd+\`` |
| Mostrar/ocultar sidebar | `Cmd+B` |
| Cambiar Terminals / Files | `Cmd+Shift+F` |
| Abrir Code Review | `Cmd+Shift+R` |
| Abrir explorador | `Cmd+Shift+O` |
| Guardar en el editor | `Cmd+S` |
| Cerrar editor | `Esc` o `Cmd+W` |
| Copiar / pegar | `Cmd+C` / `Cmd+V` |
| Limpiar | `Cmd+K` |

Linux / Windows:

| Acción | Atajo |
|---|---|
| Nueva terminal | `Ctrl+Shift+T` |
| Cerrar pane | `Ctrl+Shift+W` |
| Split derecha / abajo | `Ctrl+Shift+D` / `Ctrl+Shift+E` |
| Cambiar de terminal | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+Shift+[ / ]` |
| Mover foco entre panes | `Ctrl+Alt+flechas`, `Ctrl+Shift+\`` |
| Mostrar/ocultar sidebar | `Ctrl+Shift+B` |
| Cambiar Terminals / Files | `Ctrl+Shift+F` |
| Abrir Code Review | `Ctrl+Shift+R` |
| Abrir explorador | `Ctrl+Shift+O` |
| Guardar en el editor | `Ctrl+S` |
| Cerrar editor | `Esc` o `Ctrl+W` |
| Copiar / pegar | `Ctrl+Shift+C` / `Ctrl+Shift+V` |

## Rendimiento

- El resaltado de sintaxis vive en un hilo `syntax-highlighter`; la UI solo
  recibe el resultado final. Hay debounce de 120 ms y se descartan resultados
  obsoletos por generación/revisión.
- Los buffers de texto se cachean por pane. Solo se vuelven a dar forma a los
  panes marcados como sucios; mover el cursor, seleccionar o redibujar la
  sidebar no re-forma todo el terminal.
- El cwd se consulta cada 750 ms, no en cada evento.
- Las lecturas del PTY usan buffers de 256 KB para reducir eventos.
- Pegar texto de una sola línea se hace en una sola inserción, no carácter a
  carácter.

## Pendiente (siguientes iteraciones)

- Reordenar terminales arrastrando en la sidebar y renombrarlas.
- Indicador de actividad en terminales de fondo.
- Pantalla de ajustes real (hoy el ítem del menú es un placeholder).
- Menú nativo en Windows (Win32) si se decide; Linux usará controles propios.
- Editor: undo/redo, selección, resaltado de sintaxis y file watching.
- Code Review: PRs de GitHub, comentarios inline, resaltado en el diff,
  revisión asistida por IA y persistencia de archivos revisados.
- Más modos sobre el mismo sistema de modal.
- `modifyOtherKeys` / CSI-u (Shift+Enter y teclas modificadas).
- Overlay de preedit IME y cursor parpadeante.
- Render incremental por filas sucias y límite de parseo por frame.
- Config TOML (`~/.config/f-terminal/config.toml`) y temas.
- Emoji a color y fallback de fuentes por script.
- Tests de integración de PTY y snapshots de frames.

## Tests

```bash
cargo test
```

Incluye tests del encoder de teclado, del pipeline VT → `Frame` (texto,
colores, fondos, cursor), del árbol de splits y geometría de panes, de la
sidebar (colapsar/revelar y modo Files), del explorador de archivos, del editor
mínimo, del resaltado de sintaxis, del parser de diffs de Code Review, del
mapeo de acciones del menú y un smoke test offscreen de GPU que renderiza dos
panes y una etiqueta de UI.

## Estado

Compila y arranca en macOS (probado). Linux y Windows están soportados por el
stack elegido, pero todavía no se han probado en hardware real.
