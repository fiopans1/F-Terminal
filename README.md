# F-Terminal

Terminal nativa en Rust, ligera y multiplataforma para **macOS, Linux y
Windows**. Usa un PTY real, parser VT, render GPU con fuentes del sistema y una
sidebar estilo Warp. Incluye splits, explorador de archivos, editor con
resaltado de sintaxis, una ventana de **Code Review**, un gestor de **Git** y
un footer por terminal con la rama y los cambios del working tree.

El objetivo es tener una terminal cómoda para el día a día sin Electron ni
runtime web, manteniendo el binario y el consumo de memoria bajos.

---

## Índice

- [Características](#características)
- [Stack](#stack)
- [Requisitos](#requisitos)
- [Compilar y ejecutar](#compilar-y-ejecutar)
- [Perfiles de shell](#perfiles-de-shell)
- [Configuración](#configuración)
- [Terminal](#terminal)
- [Sidebar, sesiones y splits](#sidebar-sesiones-y-splits)
- [Explorador y editor](#explorador-y-editor)
- [Footer Git por terminal](#footer-git-por-terminal)
- [Code Review](#code-review)
- [Gestor Git](#gestor-git)
- [Atajos](#atajos)
- [Autenticación y configuración](#autenticación-y-configuración)
- [Arquitectura](#arquitectura)
- [Tests](#tests)
- [Estado y limitaciones](#estado-y-limitaciones)
- [Roadmap](#roadmap)

---

## Características

- Terminal real con PTY (Unix) y **ConPTY** (Windows), sin emulación de shell.
- Render GPU con `wgpu` y texto con `glyphon`/`cosmic-text`.
- Sidebar con árbol de sesiones y panes, más explorador de archivos.
- Sesiones plegables con panes hijos y splits derecha/abajo; divisores
  arrastrables.
- Cada pane tiene su propio shell, cwd, scrollback y estado VT.
- Selección con ratón, portapapeles, IME, bracketed paste y alternate screen.
- Colores ANSI de 16/256/truecolor, OSC 4/10/11 y consultas DA/DSR.
- Editor integrado con resaltado TextMate en background.
- **Code Review** en ventana separada: cambios locales, rama contra base,
  commits y **pull requests de GitHub o merge requests de GitLab** con buscador.
- **Gestor Git** en ventana separada: stage/unstage, commit/amend, ramas,
  fetch/pull/push, merge, stash e historial con diff.
- Footer Git por terminal con dos burbujas: rama y `+N/-N`.
- Perfiles de shell: zsh, bash, fish, `cmd.exe`, Windows PowerShell,
  PowerShell 7, Git Bash, MSYS2/Cygwin y distribuciones WSL.
- Configuración TOML con scrollback por sesión, presupuesto global opcional y
  modo de render compartido o aislado.
- La terminal principal sigue usable mientras Code Review o el gestor Git
  están abiertos.

---

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

---

## Requisitos

- Rust estable. El proyecto usa `edition = "2024"` y se ha probado con Rust
  1.100.
- Un backend gráfico disponible: Metal en macOS, Vulkan/GL en Linux, DX12 en
  Windows.
- Git instalado para Code Review, el gestor Git y el footer por terminal.
- `curl` para listar y descargar PRs de GitHub o MRs de GitLab.
- Opcional: `gh` o `glab` autenticados para reutilizar sus tokens.
- En Windows, los shells se detectan automáticamente. Git Bash debe usar
  `...\Git\bin\bash.exe`, no `git-bash.exe`.

---

## Compilar y ejecutar

```bash
# Desarrollo
cargo run

# Release optimizado
cargo run --release

# Logs de depuración
RUST_LOG=debug cargo run
```

El binario se llama `f-terminal`.

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```

La app arranca en el directorio actual. Las terminales nuevas heredan el cwd
del pane enfocado y los splits heredan también su perfil de shell.

---

## Perfiles de shell

F-Terminal detecta shells al arrancar y permite elegir uno al crear una
terminal.

### Detección en macOS y Linux

Orden de detección:

1. `[shell] default` en `config.toml`, si está definido.
2. `$SHELL`.
3. Entradas de `/etc/shells`.
4. Fallbacks `/bin/zsh`, `/bin/bash`, `/bin/sh`.

Los shells conocidos (`zsh`, `bash`, `fish`, `sh`, `dash`, `ksh`, `tcsh`…) se
lanzan con `-l` para que sean login shells y hereden el entorno del usuario.

### Detección en Windows

Se detectan, en este orden:

1. **PowerShell 7** (`pwsh.exe`), si existe.
2. **Windows PowerShell** (`powershell.exe`).
3. **Command Prompt** (`cmd.exe`, `%COMSPEC%`).
4. **Git Bash** (`...\Git\bin\bash.exe --login -i`).
5. **MSYS2 Bash** y **Cygwin Bash**, si están instalados.
6. **WSL**: perfil genérico y un perfil por distribución detectada con
   `wsl.exe -l -q`.

El perfil por defecto es el primero detectado. Si PowerShell 7 no está
instalado, se usa Windows PowerShell; si tampoco existe, `cmd.exe`.

Notas de Windows:

- Todos los shells se ejecutan bajo el mismo **ConPTY**; el resto de la app no
  cambia.
- Git Bash se lanza con `bin\bash.exe` para que use el ConPTY de F-Terminal.
- WSL recibe `--cd <ruta>` para traducir el cwd de Windows. Las rutas
  `\\wsl$\...` y `\\wsl.localhost\...` se convierten a su ruta Linux.
- `WSLENV` propaga `TERM`, `COLORTERM` y `TERM_PROGRAM` a la distribución.
- Los perfiles WSL y Git Bash heredan `TERM=xterm-256color` y
  `COLORTERM=truecolor`.

### Selector de shell

En la sidebar, junto al botón `+`, hay un botón `▾`:

- `+` crea una terminal con el perfil por defecto.
- `▾` abre la lista de perfiles dentro de la sidebar.
- `↑↓`/`Home`/`End` mueven la selección, `Enter` crea la terminal y `Esc`
  cierra el selector.
- Un clic sobre un perfil crea la terminal con ese shell.
- Atajo: `Cmd+Shift+N` en macOS, `Ctrl+Shift+N` en Linux/Windows.

Los splits heredan el perfil del pane enfocado. La lista muestra el nombre del
shell y una etiqueta corta (`zsh`, `bash`, `cmd`, `PS`, `pwsh`, `WSL`).

### Shell por defecto

El campo `[shell] default` de `config.toml` permite forzar un shell por ruta o
nombre. Si se omite, se usa la detección automática descrita arriba.

---

## Configuración

F-Terminal usa un único fichero `config.toml`. Se crea automáticamente en el
primer arranque con una plantilla comentada y permisos `600` en Unix.

| Plataforma | Ruta |
|---|---|
| macOS / Linux | `~/.config/f-terminal/config.toml` |
| Linux con `XDG_CONFIG_HOME` | `$XDG_CONFIG_HOME/f-terminal/config.toml` |
| Windows | `%APPDATA%\f-terminal\config.toml` |

La variable `F_TERMINAL_CONFIG` permite usar otra ruta.

El fichero se lee al arrancar: reinicia la app para aplicar cambios. Si el TOML
no se puede leer o parsear, se usan los valores por defecto y se escribe un
aviso en el log. Los números fuera de rango se recortan; un `render.mode`
desconocido cae a `isolated` y un tema desconocido a Tokyo Night.

### Ejemplo completo

```toml
[appearance]
# font_family = "SF Mono"
font_size = 14.0
line_height = 1.3
padding = 10.0
sidebar_width = 210.0
ui_font_size = 12.5
# El valor por defecto depende de la plataforma:
# false en macOS, true en Linux/Windows.
option_as_meta = false
theme = "tokyo-night"

[terminal]
# Historial por pane en la sesión activa.
scrollback_lines = 6000
# Historial por pane en sesiones ocultas.
hidden_scrollback_lines = 3000
# Presupuesto global de scrollback en MB. 0 lo desactiva.
scrollback_budget_mb = 0

[render]
# "shared" comparte GPU y atlas de texto entre ventanas.
# "isolated" crea recursos propios en cada ventana.
mode = "shared"

[shell]
# default = "zsh"

[forge.github]
# token = "github_pat_..."

[forge.gitlab]
# token = "glpat-..."
```

### Scrollback

- `scrollback_lines` es el límite por pane de la sesión activa. Por defecto
  **6000** líneas. Se recorta al rango `500..100000`.
- `hidden_scrollback_lines` es el límite por pane de las sesiones que no están
  activas. Por defecto **3000** líneas. Se recorta a `100..scrollback_lines`.
- «Oculta» significa que su pestaña/sesión no es la activa; plegar una sesión
  en la sidebar no reduce su historial.
- Al cambiar de sesión, sus panes suben al límite activo; al ocultarse bajan al
  límite oculto y se descarta el historial más antiguo que exceda el nuevo
  límite.
- `scrollback_budget_mb = 0` desactiva el presupuesto global, que es el valor
  por defecto. Se recorta al rango `0..4096`. Si se activa, se estima
  `columnas × líneas × 32 bytes` por pane y se recorta primero el historial de
  sesiones ocultas, después el de panes visibles no enfocados y por último el
  del pane enfocado. Ningún pane baja de **1000** líneas. Si el presupuesto es
  imposible de cumplir, se mantiene el mínimo y se registra un aviso.

Como referencia, un pane de 120 columnas con 6000 líneas se estima en unos
23 MB de historial.

### Render compartido o aislado

- `mode = "shared"` (por defecto): la ventana principal crea un único
  `Instance`/`Adapter`/`Device`/`Queue` y una única caché de fuentes y atlas de
  glifos. Code Review y el gestor Git reutilizan esos recursos, manteniendo su
  propia superficie, viewport, factor de escala y métricas de celda. Los atlas
  se separan por formato de textura si las superficies no coinciden.
- `mode = "isolated"`: cada ventana crea su propio renderer, dispositivo y
  recursos de texto. Es el comportamiento anterior y la opción conservadora si
  aparecen problemas de glifos o DPI.
- Si el render compartido falla al abrir una ventana secundaria, se registra un
  warning y esa ventana cae automáticamente a renderer aislado; la app sigue
  funcionando.
- El modo se aplica al arrancar. Cambiarlo requiere reiniciar la app.

### Volver al comportamiento anterior

Para restaurar el scrollback y el renderer previos, usa:

```toml
[terminal]
scrollback_lines = 10000
hidden_scrollback_lines = 10000
scrollback_budget_mb = 0

[render]
mode = "isolated"
```

y reinicia F-Terminal.

---

## Terminal

Cada pane tiene:

- Su propio PTY y proceso hijo.
- Su propio `Term` de `alacritty_terminal` y su `Frame` renderizable.
- Scrollback independiente.
- Su cwd, que se sigue mediante `sysinfo` cada 750 ms.
- Su estado de selección, cursor y modos VT.

Soporte VT incluido:

- ANSI de 16/256 colores y truecolor.
- Atributos inverse, hidden, bold, underline, etc.
- Alternate screen y scrollback.
- Respuestas DA/DSR y consultas de color.
- Título de ventana por OSC.
- Foco, IME commit y bracketed paste.
- Ratón SGR y rueda.
- Redimensionado real del PTY (`TIOCSWINSZ`/ConPTY).

El renderer no conoce `alacritty_terminal`: consume un `Frame` ya resuelto con
texto, colores, fondos y cursor. Eso mantiene aislado el motor VT de la capa
gráfica.

---

## Sidebar, sesiones y splits

La sidebar tiene dos modos:

- **SESSIONS**: árbol de sesiones/workspaces y sus panes.
- **FILES**: explorador del cwd del pane enfocado.

### Árbol de sesiones

Cada sesión es un workspace con uno o más panes:

- Una sesión con un solo pane ocupa una fila.
- Cuando la sesión tiene varios panes, aparece una fila padre y los panes como
  hijos, con conectores `├`/`└`.
- El padre muestra un chevron `▾`/`▸` para plegar o desplegar sus hijos y un
  contador de panes.
- El título inicial de una sesión es **New session**. Cuando el shell envía su
  título por OSC, el padre se actualiza.
- Los hijos muestran el título del pane o, si no hay, el shell (`zsh`, `bash`,
  `PowerShell`…).
- Clic en el padre: enfoca el pane activo de esa sesión.
- Clic en un hijo: enfoca ese pane concreto.
- `×` en el padre cierra la sesión completa.
- `×` en un hijo cierra solo ese pane.
- El pane enfocado se resalta con una barra de acento.

Acciones:

- `+` crea una nueva sesión con el shell por defecto.
- `▾` abre el selector de perfiles.
- `F` cambia a FILES; `T` vuelve a SESSIONS.
- `«` colapsa la sidebar; el rail `»` la recupera.
- Rueda del ratón para hacer scroll si hay muchas sesiones o panes.

Dentro de cada sesión:

- `Cmd+D` / `Ctrl+Shift+D`: split a la derecha.
- `Cmd+Shift+D` / `Ctrl+Shift+E`: split abajo.
- Los nuevos panes aparecen como hijos del árbol y expanden la sesión.
- Los divisores se arrastran con el ratón.
- El pane enfocado muestra un borde de acento.
- `Cmd+Alt+flechas` / `Ctrl+Alt+flechas`: mover el foco.
- `Cmd+\`` / `Ctrl+Shift+\``: ciclar panes.

---

## Explorador y editor

### Explorador

- Sigue el cwd real del shell enfocado.
- Carpetas expandibles con `▸`/`▾`.
- Archivos abribles con un clic.
- Botón `↻` para refrescar.
- No usa file watching: se actualiza al hacer `cd` o al refrescar.

### Editor

- Panel a la derecha de la terminal.
- Divisor arrastrable para redimensionar.
- `Cmd+S` / `Ctrl+S` guarda.
- `Esc` o `Cmd+W` cierra. El punto de acento indica cambios sin guardar.
- Resaltado TextMate con `syntect` en un hilo aparte, con debounce de 120 ms.
- Los buffers de texto se cachean por pane y solo se re-shapean cuando cambian.

Limitaciones actuales del editor:

- Sin undo/redo ni selección de texto.
- Sin file watching ni autoguardado.
- Sin resaltado en vivo para archivos grandes (>512 KB).
- Solo UTF-8 y archivos de hasta 2 MB.
- Los tabs se expanden a 4 espacios al cargar.
- Cerrar el editor descarta cambios no guardados.

---

## Footer Git por terminal

Cada pane muestra dos burbujas abajo a la izquierda:

1. Rama actual, con un punto de acento.
2. Cambios del working tree: `+N -N`, en una sola píldora con la mitad verde
   para adiciones y la mitad roja para eliminaciones.

Detalles:

- Usa `git diff --numstat HEAD`, así que cuenta cambios **tracked** staged y
  unstaged. Los archivos untracked no suman.
- Se actualiza al cambiar el cwd y cada 2 segundos.
- La inspección Git corre en un hilo de background por pane.
- El ancho de las burbujas se mide con `cosmic-text`, no con una estimación por
  caracteres.
- Fuera de un repositorio Git no se dibujan las burbujas.
- Los clics y la rueda dentro de la zona del footer no se envían al PTY.

---

## Code Review

Se abre con `Cmd+Shift+R` / `Ctrl+Shift+R` o desde el menú de macOS.

Es una **ventana nativa independiente**: la terminal principal sigue usable.

### Pestaña LOCAL

- **Working changes**: cambios locales, incluidos untracked.
- **Branch vs main/master**: diff de la rama actual contra la base.
- **Últimos 15 commits**.
- `↑↓` para mover, `Enter` o clic para abrir, `R` para refrescar.

### Pestaña PULL / MERGE REQUESTS

- Detecta el proveedor a partir del remoto `origin`:
  - **GitHub**: la pestaña se llama **PULL REQUESTS** y usa `#123`.
  - **GitLab**: la pestaña se llama **MERGE REQUESTS** y usa `!123`.
- Lista los cambios abiertos del remoto.
- Carga paginada de hasta 1.000 PRs/MRs.
- Buscador por número, título, autor o rama; admite varios términos separados
  por espacios.
- `/` o `Cmd+F`/`Ctrl+F` enfocan el buscador.
- `↑↓` selecciona, `Enter` carga el diff, `R` recarga la lista.
- `Esc` sale del buscador sin cerrar la ventana.

Soporte de remotos:

- `github.com` y GitHub Enterprise en hosts `github.*`.
- `gitlab.com`, grupos anidados (`grupo/subgrupo/repo`) y GitLab self-hosted en
  hosts `gitlab.*`.
- Los hosts con otros nombres necesitarán configuración explícita en una
  iteración futura.

### Diff y revisión

- Tres columnas: reviews, selector de archivos y diff.
- Estado por archivo: `M/A/D/R/B`.
- `+adiciones −eliminaciones` por archivo.
- `Espacio` o `M` marca un archivo como revisado.
- Las marcas viven solo en memoria durante la sesión.
- `Tab` cambia de panel; `Esc` o `×` cierran.
- El diff se carga en background y se descarta si llega tarde
  (`window_id` + `request_id`).

### Autenticación de GitHub y GitLab

Los repos públicos funcionan sin token, con el límite de peticiones de cada
API. Para repos privados:

**GitHub**, por orden:

1. `[forge.github] token` en `config.toml`.
2. `GITHUB_TOKEN` o `GH_TOKEN` en el entorno.
3. `~/.config/f-terminal/github-token`
   (`%APPDATA%\f-terminal\github-token` en Windows), con el token en la primera
   línea.
4. `gh auth token`, si `gh` está instalado y autenticado.

Token recomendado:

- Fine-grained: lectura de **Pull requests** y **Contents**.
- Classic: scope `repo`.

**GitLab**, por orden:

1. `[forge.gitlab] token` en `config.toml`.
2. `GITLAB_TOKEN` o `GL_TOKEN` en el entorno.
3. `~/.config/f-terminal/gitlab-token`
   (`%APPDATA%\f-terminal\gitlab-token` en Windows).
4. `glab auth token`, si `glab` está instalado y autenticado.

Token recomendado: scope **`read_api`**.

Los tokens se envían a `curl` por stdin para no exponerlos en la lista de
procesos.

---

## Gestor Git

Se abre con `Cmd+Shift+G` / `Ctrl+Shift+G` o desde el menú de macOS.

Es otra **ventana nativa independiente**, distinta de Code Review. Puede estar
abierta a la vez que la terminal y que Code Review. Usa el repositorio Git del
cwd del pane enfocado.

### Interfaz

```
┌─────────────────────────────────────────────────────────────────────┐
│ GIT  repo · main ↑2 ↓1        Fetch  Pull  Push  Refresh            │
├──────────────┬──────────────────────────────────┬───────────────────┤
│ CHANGES      │ HISTORIA                         │ DETALLE           │
│ Staged (3)   │ ● abc1234  Fix footer...  main   │ autor, fecha      │
│ Changes (5)  │ ● def5678  Add PR search         │ mensaje           │
│              │ ● 9012abc  ...                   │ archivos          │
│ REFS         │                                  │ diff              │
│  ramas       │                                  │                   │
│  remotos     │                                  │                   │
│  tags        │                                  │                   │
│  stashes     │                                  │                   │
├──────────────┴──────────────────────────────────┴───────────────────┤
│ Mensaje del commit...                          Stash Amend Commit    │
└─────────────────────────────────────────────────────────────────────┘
```

### Working tree

- Lista separada de **staged** y **unstaged**.
- Marca de conflicto `U` para archivos sin resolver.
- Clic en `+` para stage, `−` para unstage.
- `Enter`/`Espacio` sobre una fila también hace stage/unstage.
- `Supr`/`Backspace` descarta cambios con confirmación.
- Botones **Stage all** / **Unstage all**.
- Si hay conflictos aparece **Abort merge** (`git merge --abort`).

### Commits

- Campo de mensaje de una línea en la barra inferior.
- **Commit** solo con staged y mensaje no vacío.
- **Amend** activa `git commit --amend`.
- El mensaje se pasa a Git por stdin (`git commit -F -`), sin shell.
- Los commits se hacen en background y muestran el resultado en el footer.

### Ramas, remotos y stash

En la pestaña **REFS**:

- `↑↓` mueve la selección.
- `Enter` hace checkout de una rama local o remota.
- Una rama remota sin local crea una rama de tracking.
- `N` abre el diálogo de nueva rama.
- `M` hace merge de la rama seleccionada.
- `D` borra una rama local con confirmación.
- En stashes, `Pop` aplica y `Drop` elimina con confirmación.
- `R` refresca.

### Remoto

- **Fetch**: `git fetch --all --prune`.
- **Pull**: `git pull --ff-only` para evitar merges inesperados.
- **Push**: `git push`; si no hay upstream, `git push -u origin <rama>`.
- Los comandos de red se ejecutan en background y no bloquean la UI.

### Historial y detalle

- Historial de hasta 300 commits con working tree como primera fila.
- Selección con `↑↓`, `PgUp/PgDn`, `Home/End`.
- Detalle del commit: hash, asunto, autor, fecha y refs.
- Lista de archivos con estado y `+/-`.
- Diff por archivo con numeración y colores.
- `↑↓` cambia de archivo; `PgUp/PgDn` desplaza el diff; `←→` horizontal.

### Refresco

- Working tree cada 2 segundos.
- Historial, refs y detalle tras cada operación o con `R`.
- Todas las operaciones mutantes se serializan: mientras una corre, el resto se
  deshabilita.

### Limitaciones del gestor Git

- Sin rebase, cherry-pick, revert, gestión de tags ni force push.
- Borrar rama usa `-d`; si no está mergeada, Git devuelve error.
- Pull solo fast-forward.
- Conflictos: se muestran y se pueden abortar, pero no hay editor de
  resolución integrado.
- El historial se limita a 300 commits.
- No hay autenticación interactiva: Git debe tener configurados SSH agent o
  credential helper. Se ejecuta con `GIT_TERMINAL_PROMPT=0`.
- No hay push forzado ni gestión de remotos.

---

## Atajos

### Globales — macOS

| Acción | Atajo |
|---|---|
| Nueva terminal (perfil por defecto) | `Cmd+T` |
| Elegir perfil de shell | `Cmd+Shift+N` |
| Cerrar pane / terminal | `Cmd+W` / `Cmd+Shift+W` |
| Split derecha / abajo | `Cmd+D` / `Cmd+Shift+D` |
| Cambiar de terminal | `Cmd+Shift+[` / `]`, `Cmd+1..9`, `Ctrl+Tab` |
| Mover foco entre panes | `Cmd+Alt+flechas`, `Cmd+\`` |
| Mostrar/ocultar sidebar | `Cmd+B` |
| Cambiar SESSIONS / FILES | `Cmd+Shift+F` |
| Abrir Code Review | `Cmd+Shift+R` |
| Abrir gestor Git | `Cmd+Shift+G` |
| Abrir explorador | `Cmd+Shift+O` |
| Guardar en el editor | `Cmd+S` |
| Cerrar editor | `Esc` o `Cmd+W` |
| Copiar / pegar | `Cmd+C` / `Cmd+V` |
| Limpiar terminal | `Cmd+K` |

### Globales — Linux / Windows

| Acción | Atajo |
|---|---|
| Nueva terminal (perfil por defecto) | `Ctrl+Shift+T` |
| Elegir perfil de shell | `Ctrl+Shift+N` |
| Cerrar pane | `Ctrl+Shift+W` |
| Split derecha / abajo | `Ctrl+Shift+D` / `Ctrl+Shift+E` |
| Cambiar de terminal | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+Shift+[ / ]` |
| Mover foco entre panes | `Ctrl+Alt+flechas`, `Ctrl+Shift+\`` |
| Mostrar/ocultar sidebar | `Ctrl+Shift+B` |
| Cambiar SESSIONS / FILES | `Ctrl+Shift+F` |
| Abrir Code Review | `Ctrl+Shift+R` |
| Abrir gestor Git | `Ctrl+Shift+G` |
| Abrir explorador | `Ctrl+Shift+O` |
| Guardar en el editor | `Ctrl+S` |
| Cerrar editor | `Esc` o `Ctrl+W` |
| Copiar / pegar | `Ctrl+Shift+C` / `Ctrl+Shift+V` |
| Limpiar terminal | `Ctrl+Shift+K` |

### Selector de shell

| Acción | Tecla |
|---|---|
| Mover selección | `↑` / `↓` |
| Ir al primero / último | `Home` / `End` |
| Crear terminal con el perfil | `Enter` |
| Cerrar selector | `Esc` |

### Code Review

| Acción | Tecla |
|---|---|
| Mover por reviews/PRs/archivos | `↑` / `↓`, `PgUp` / `PgDn` |
| Abrir review o PR | `Enter` |
| Buscar PRs | `/` o `Cmd/Ctrl+F` |
| Marcar archivo revisado | `Espacio` o `M` |
| Cambiar de panel | `Tab` |
| Refrescar | `R` |
| Salir del buscador | `Esc` |
| Cerrar ventana | `Esc` o `×` |

### Gestor Git

| Acción | Tecla |
|---|---|
| Cambiar de panel | `Tab` |
| Navegar listas | `↑` / `↓`, `PgUp` / `PgDn`, `Home` / `End` |
| Stage / unstage | `Enter` o `Espacio` |
| Descartar cambios | `Supr` o `Backspace` |
| Checkout / abrir stash | `Enter` |
| Nueva rama | `N` |
| Merge | `M` |
| Borrar rama | `D` |
| Refrescar | `R` |
| Commit | `Enter` en el campo de mensaje |
| Cerrar ventana | `Esc` o `Cmd/Ctrl+W` |

---

## Autenticación y configuración

### Git local y remoto

El gestor Git y el footer usan el binario `git` del sistema. Para operaciones
de red se asume que el usuario ya tiene configurados:

- SSH agent y claves para `git@github.com`.
- O un credential helper para HTTPS.

Los comandos se ejecutan sin `sh -c`, con `GIT_TERMINAL_PROMPT=0`,
`GIT_PAGER=cat` y `LC_ALL=C`. Si Git necesita una contraseña interactiva, la
operación falla y el error se muestra en el footer de la ventana Git.

### GitHub y GitLab para Code Review

La pestaña de cambios remotos usa la API REST del proveedor detectado. Los
repos públicos no necesitan token. Para repos privados, configura un token como
se describe arriba.

Ejemplos:

```bash
# GitHub
export GITHUB_TOKEN=ghp_...

# GitLab
export GITLAB_TOKEN=glpat-...
```

O crea el fichero correspondiente:

```bash
mkdir -p ~/.config/f-terminal
printf 'TU_TOKEN\n' > ~/.config/f-terminal/github-token
printf 'TU_TOKEN\n' > ~/.config/f-terminal/gitlab-token
chmod 600 ~/.config/f-terminal/github-token ~/.config/f-terminal/gitlab-token
```

En Windows:

```
%APPDATA%\f-terminal\github-token
%APPDATA%\f-terminal\gitlab-token
```

También se pueden usar `gh auth token` y `glab auth token` si esos CLIs están
instalados y autenticados.

### Configuración

La configuración general, incluidos los tokens, se describe en
[Configuración](#configuración).

---

## Arquitectura

```
src/
  main.rs          Arranque, config y event loop
  app.rs           Ventana principal, routing multi-ventana, atajos y estado
  config.rs        Configuración TOML, validación y plantilla
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
    mod.rs         Term + parser + selección
    color.rs       Paleta y conversión sRGB
    frame.rs       Snapshot del grid
  render/
    mod.rs         Superficie wgpu y recursos GPU/texto compartidos
    scene.rs       Panes, labels y quads
    quads.rs       Rectángulos/píldoras instanciados
    text.rs        Buffers, glyphon y atlas compartido
    quads.wgsl     Shader de quads con esquinas redondeadas
```

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
- Los resultados llevan `request_id` para descartar respuestas obsoletas.
- Las operaciones mutantes de Git se serializan con un estado `busy`.
- Los tokens de GitHub y GitLab se pasan a `curl` por stdin.

---

## Tests

```bash
cargo test
```

Cobertura actual (54 tests):

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
- Mapeo de acciones del menú.
- Smoke test offscreen de GPU con dos panes, una etiqueta y un atlas de texto
  compartido entre dos ventanas.

Comprobaciones recomendadas antes de un cambio:

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

---

## Estado y limitaciones

Probado en macOS. Linux y Windows están soportados por el stack y compilan,
pero no se han validado en hardware real en este entorno.

Limitaciones conocidas:

- Sin pantalla de ajustes ni recarga en caliente del TOML.
- Editor sin undo/redo, selección, file watching ni autoguardado.
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
- Sin búsqueda en el scrollback.
- Sin pestañas dentro de una misma ventana más allá del workspace.
- Shells de Windows implementados por detección y ConPTY, pero pendientes de
  prueba manual en Windows.
- En WSL, rutas UNC que no sean `\\wsl$` o `\\wsl.localhost` pueden requerir
  ajustes manuales.
- Sin emoji a color ni fallback avanzado por script.
- Sin render incremental por filas sucias ni límite de parseo por frame.

---

## Roadmap

- Validar perfiles de shell y ConPTY en Windows real.
- Pantalla de ajustes, recarga en caliente del TOML y perfiles de shell
  personalizados.
- Selector de tema y más paletas.
- Añadir más perfiles: NuShell, fish en Windows, distros WSL concretas.
- Git: tags, rebase, cherry-pick, revert, force push con lease y resolución de
  conflictos.
- Code Review: Bitbucket/Azure DevOps, comentarios inline, submit de reviews y
  resaltado en el diff.
- Config por proveedor para hosts self-hosted con nombres arbitrarios.
- Aprobaciones y discusiones de MRs/PRs.
- Editor: undo/redo, selección, file watching y autoguardado.
- Sidebar: reordenar arrastrando, renombrar y actividad en segundo plano.
- Búsqueda en scrollback.
- Menú nativo en Windows/Linux.
- Mejoras de rendimiento: render incremental y menos re-shaping.
- Tests de integración de PTY y snapshots de frames.
