# F-Terminal

[![CI](https://github.com/fiopans1/F-Terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/fiopans1/F-Terminal/actions/workflows/ci.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg)](#requisitos)
[![Status](https://img.shields.io/badge/status-early%20development-yellow.svg)](#estado-y-limitaciones)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

Terminal nativa en Rust, ligera y multiplataforma para **macOS, Linux y
Windows**. Usa un PTY real, parser VT, render GPU con fuentes del sistema y una
sidebar estilo Warp. Incluye splits, explorador de archivos, editor con
resaltado de sintaxis, una ventana de **Code Review**, un gestor de **Git** y
un footer por terminal con la rama y los cambios del working tree.

El objetivo es tener una terminal cómoda para el día a día sin Electron ni
runtime web, manteniendo el binario y el consumo de memoria bajos.

La documentación está en español. Issues and pull requests in English are
welcome.

---

## Índice

- [Características](#características)
- [Stack](#stack)
- [Requisitos](#requisitos)
- [Compilar y ejecutar](#compilar-y-ejecutar)
- [Instaladores y releases](#instaladores-y-releases)
- [Perfiles de shell](#perfiles-de-shell)
- [Configuración](#configuración)
- [Terminal](#terminal)
- [Autocompletado](#autocompletado)
- [Sidebar, sesiones y splits](#sidebar-sesiones-y-splits)
- [Explorador y editor](#explorador-y-editor)
- [Footer Git por terminal](#footer-git-por-terminal)
- [Code Review](#code-review)
- [Gestor Git](#gestor-git)
- [Agent Mode](#agent-mode)
- [Atajos](#atajos)
- [Autenticación y configuración](#autenticación-y-configuración)
- [Arquitectura](#arquitectura)
- [Tests](#tests)
- [Estado y limitaciones](#estado-y-limitaciones)
- [Roadmap](#roadmap)
- [Contribuir](#contribuir)
- [Seguridad](#seguridad)
- [Changelog](#changelog)
- [Licencia](#licencia)

---

## Características

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

### Rendimiento

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

- Rust estable, mínimo **1.95**. El proyecto usa `edition = "2024"` y se ha
  probado con Rust 1.100.
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

La app arranca en el directorio actual cuando se lanza desde una terminal; si
se abre desde Finder, el menú Inicio o un lanzador gráfico, usa el directorio
home del usuario. Las terminales nuevas heredan el cwd del pane enfocado y los
splits heredan también su perfil de shell.

---

## Instaladores y releases

F-Terminal se empaqueta de forma nativa para cada plataforma. Los artefactos
se generan en `dist/` y **no descargan nada durante la instalación**.

### GitHub Actions: solo tags

El workflow [`.github/workflows/installers.yml`](.github/workflows/installers.yml)
se ejecuta únicamente cuando se empuja un tag `v*`:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Antes de compilar, valida que el tag coincida exactamente con el campo
`version` de `Cargo.toml`. Después compila en runners nativos y publica un
GitHub Release con:

- `F-Terminal-0.1.0-macos-universal.dmg`
- `F-Terminal-0.1.0-macos-universal.pkg`
- `F-Terminal-Setup-0.1.0-x64.exe`
- `f-terminal_0.1.0-1_amd64.deb`
- `f-terminal-0.1.0-1.x86_64.rpm`
- `SHA256SUMS`

El workflow no se ejecuta en cada push ni mediante `workflow_dispatch`.

Para probar el pipeline sin publicar una versión estable, usa una prerelease:

```bash
git tag v0.1.0-alpha-001-test
git push origin v0.1.0-alpha-001-test
```

El workflow valida que `v0.1.0` coincida con `Cargo.toml`, genera todos los
instaladores y publica el resultado como **prerelease** de GitHub.

macOS se compila en `macos-latest`. Si el runner de macOS no está disponible
(por ejemplo, por límites de minutos en un repositorio privado), la release
publica los artefactos de Linux/Windows y deja un warning; para generar el DMG
y el PKG hace falta un runner macOS, una cuenta con minutos o un runner
self-hosted.

### Build local

```bash
# macOS
./scripts/package.sh macos

# macOS con instalador .pkg además del .dmg
./scripts/package.sh macos --pkg

# Linux (Debian/Ubuntu o Fedora/RHEL)
./scripts/package.sh linux

# Windows
powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
```

Requisitos de build:

- macOS: Xcode Command Line Tools, Rust y los targets
  `aarch64-apple-darwin` y `x86_64-apple-darwin`.
- Windows: Rust MSVC, Visual Studio Build Tools e Inno Setup 6.
- Linux: Rust, `dpkg-deb`, `rpmbuild` y `file`.

El script de macOS genera un binario universal `arm64 + x86_64`. Si no hay
identidad de firma configurada, firma en modo ad-hoc; para distribución pública
se puede usar una identidad Developer ID y notarización:

```bash
CODESIGN_IDENTITY="Developer ID Application: ..." \
APPLE_ID="..." APPLE_TEAM_ID="..." APPLE_APP_PASSWORD="..." \
./scripts/package.sh macos
```

El instalador de Windows se ejecuta **siempre para todos los usuarios**, con
UAC y destino en `C:\Program Files\F-Terminal`. Se publica sin firma
Authenticode, así que SmartScreen puede mostrar un aviso de editor desconocido.
Para evitarlo hace falta un certificado OV/EV emitido por una CA de confianza.

### Instalación para usuarios

```bash
# macOS
open dist/macos/F-Terminal-0.1.0-macos-universal.dmg

# Windows
dist\windows\F-Terminal-Setup-0.1.0-x64.exe

# Debian / Ubuntu
sudo apt install ./dist/linux/f-terminal_0.1.0-1_amd64.deb

# Fedora / RHEL
sudo dnf install ./dist/linux/f-terminal-0.1.0-1.x86_64.rpm
```

Los instaladores de Windows se publican sin firma, así que SmartScreen puede
mostrar sus avisos habituales. Los `.deb`/`.rpm` usan las librerías base del
sistema; no se incluye AppImage en esta iteración.

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
- `WSLENV` propaga `TERM`, `COLORTERM`, `TERM_PROGRAM` y
  `MISE_IGNORED_CONFIG_PATHS` a la distribución. Esto último evita que el
  `mise` de Linux cargue el config global de Windows cuando el pane arranca bajo
  `C:\Users\...` y falle porque no está trusted.
- En Windows, F-Terminal pide la ruta al propio shell mediante `OSC 9;9`:
  PowerShell 7 y Windows PowerShell envuelven su función `prompt`, `cmd.exe`
  usa `$E]9;9;$P` en `PROMPT`, y Git Bash/MSYS2/Cygwin añaden un
  `PROMPT_COMMAND`. Así los splits, las pestañas nuevas, el explorador y el
  footer Git siguen el `cd` actual aunque el proceso no actualice su cwd
  (PowerShell es el caso más claro).
- Si `git` no está en el `PATH` del proceso, se busca en las instalaciones
  habituales de Git for Windows, Scoop y Chocolatey antes de rendirse.
- WSL no puede leer el cwd de su shell Linux desde Windows; los perfiles WSL
  siguen usando la última ruta traducida que F-Terminal conoce.
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

`Cmd+Shift+S` / `Ctrl+Shift+S` abre el `config.toml` en el editor integrado. Los
cambios se aplican en caliente en cuanto se guarda el fichero: tema, tamaños de
letra, márgenes, scrollback, perfil de shell por defecto, agentes y el resto de
opciones de comportamiento. Los cambios estructurales —`font_family` y
`render.mode`— siguen necesitando reiniciar la app; el resto se aplica sin
perder las sesiones abiertas.

Si el TOML no se puede leer o parsear durante una recarga, se conserva la
configuración anterior (no se restablecen los valores por defecto) y se escribe
un aviso en el log. Al arrancar, un fichero inválido sí cae a los valores por
defecto. Los números fuera de rango se recortan; un `render.mode` desconocido
cae a `isolated` y un tema desconocido a Foundry.

### Ejemplo completo

```toml
[appearance]
# font_family = "SF Mono"
font_size = 14.0
line_height = 1.3
# Sin marco: la terminal llega hasta los bordes del panel.
padding = 0.0
# Margen izquierdo extra para el texto del terminal.
padding_left = 8.0
# false oculta el prefijo user@host del prompt (en cualquier SO).
show_user_in_prompt = true
sidebar_width = 210.0
ui_font_size = 12.5
# El valor por defecto depende de la plataforma:
# false en macOS, true en Linux/Windows.
option_as_meta = false
theme = "foundry"

[terminal]
# Historial por pane en la sesión activa.
scrollback_lines = 6000
# Historial por pane en sesiones ocultas.
hidden_scrollback_lines = 3000
# Presupuesto global de scrollback en MB. 0 lo desactiva.
scrollback_budget_mb = 0
# Autocompletado. Desactiva las fuentes que no quieras.
completions = true
completion_history = true
completion_commands = true
completion_paths = true
completion_git = true
# Caracteres mínimos para mostrar texto fantasma.
completion_min_chars = 2

[render]
# "shared" comparte GPU y atlas de texto entre ventanas.
# "isolated" crea recursos propios en cada ventana.
mode = "shared"

[session]
# Restaura pestañas, splits, directorios y geometría al abrir.
restore = true
# Restaura también el scrollback de cada pane.
restore_scrollback = true

[agents]
# Descubre agentes y responde a OSC 7501.
enabled = true
# Incluye agentes de otras terminales/IDEs (solo lectura).
show_external = true
# Intervalo del escaneo de procesos, en milisegundos.
poll_interval_ms = 2000

[shell]
# default = "zsh"

[forge.github]
# token = "github_pat_..."

[forge.gitlab]
# token = "glpat-..."
```

### Margen izquierdo

`padding_left` añade espacio solo a la izquierda del grid del terminal (8 px por
defecto), para que el texto no quede pegado a la sidebar. El resto de lados
sigue sin marco; `padding` sigue controlando un inset en los cuatro lados si se
quiere.

### Identidad visual

**Foundry** es la identidad propia de F-Terminal: superficies de grafito con un
punto de verde petróleo y un acento cálido de latón. El latón marca la acción
principal, el foco y la rama actual; el verde petróleo queda para el trabajo en
curso y los estados informativos. No es una variante de GitHub Dark: el
contraste cálido/frío busca que la app se reconozca de un vistazo.

La carcasa de las ventanas Git, Code Review y Agentes comparte alturas
(cabecera de 56 px, footer de 28 px), una escala tipográfica mínima de 11 px,
botones de 30 px, foco visible y copy en español. Los estados vacíos explican
qué hacer a continuación y las listas largas muestran scrollbar.

### Temas

`theme` selecciona la paleta completa (terminal, sidebar, diffs y Code Review):

| Valor | Tema |
|---|---|
| `foundry` (por defecto) | Foundry |
| `github-dark` | GitHub Dark |
| `tokyo-night` | Tokyo Night |
| `catppuccin-mocha` | Catppuccin Mocha |
| `dracula` | Dracula |
| `nord` | Nord |
| `gruvbox-dark` | Gruvbox Dark |
| `one-dark` | One Dark |
| `solarized-dark` | Solarized Dark |
| `kanagawa` | Kanagawa |

Se aceptan guiones o guiones bajos y algunos alias (`gruvbox`, `onedark`,
`solarized`, `github`). Un nombre desconocido cae en Foundry y se escribe un
aviso en el log.

### Prefijo `user@host`

Con `show_user_in_prompt = false`, F-Terminal oculta el prefijo `user@host`
del prompt visible, incluidas las líneas ya impresas en pantalla, y de los
títulos de pane/ventana (por ejemplo `fiopans1@MacBook-Pro ~/repo %` pasa a
`~/repo %`). Funciona en macOS, Linux y Windows, conserva los colores del resto
del prompt y no modifica tu `PS1`/`PROMPT` ni el historial del shell.

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
- Un aviso **Iniciando <perfil>…** hasta que el shell produce su primera salida.

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

### Búsqueda en scrollback y enlaces

`Cmd+Shift+F` en macOS y `Ctrl+Shift+F` en Linux/Windows abren el buscador del
pane enfocado. La búsqueda recorre el scrollback y la pantalla visible, trata la
consulta como texto literal y usa *smart case*: en minúsculas ignora
mayúsculas; si escribes alguna mayúscula, distingue. Los resultados se resaltan
y se limitan a 2000 coincidencias para mantener la interfaz fluida.

| Acción | Tecla |
|---|---|
| Abrir/cerrar el buscador | `Cmd+Shift+F` / `Ctrl+Shift+F` |
| Buscar mientras escribes | cualquier texto |
| Siguiente / anterior | `Enter` / `Shift+Enter` o `↓` / `↑` |
| Borrar un carácter | `Backspace` |
| Cerrar | `Esc` |

El contador del cuadro muestra `posición/total`. Al abrir otro pane, cambiar de
sesión o cerrar el pane buscado, el buscador se cierra.

Los enlaces `OSC 8` y las URLs planas (`http://`, `https://`, `www.`, `mailto:`)
se detectan al pasar el ratón y se subrayan con el color de acento. Para evitar
aperturas accidentales, `Ctrl+Click` en Linux/Windows o `Cmd+Click` en macOS
abre el enlace en el navegador predeterminado. Solo se permiten esos esquemas;
`file://` y otros se ignoran.

---

## Autocompletado

Cada terminal ofrece sugerencias mientras escribes en el prompt:

- **Historial**: comandos ya ejecutados, incluida la sesión actual.
- **Comandos**: ejecutables del `PATH` y builtins comunes.
- **Subcomandos de Git**: `checkout`, `switch`, `rebase`, etc.
- **Rutas**: archivos y directorios, relativos, absolutos y con `~`.
- **Git**: ramas locales, remotas y tags, solo cuando el subcomando las acepta
  (`git checkout`, `switch`, `merge`, `rebase`, `push`, `log`...).

La mejor sugerencia aparece como texto atenuado detrás del cursor:

| Acción | Atajo |
|---|---|
| Aceptar texto fantasma | `→` |
| Abrir/cerrar el popup | `Ctrl+.`, `Ctrl+Space` o `Ctrl+Shift+Space` |
| Mover selección en el popup | `↑` / `↓` |
| Insertar candidato | `Tab` o `Enter` |
| Cerrar el popup | `Esc` |

`Tab` sin popup se sigue enviando al shell, para no perder el completado
específico de zsh/bash/fish o de herramientas como `docker` y `kubectl`.

El motor es deliberadamente ligero:

- Historial en un único buffer con rangos, limitado a 5.000 comandos / 512 KB.
- Índice de comandos del `PATH` construido una vez en segundo plano.
- Rutas: solo se lista el directorio del token actual, con caché de 2 s.
- Git: lectura directa de `refs/heads`, `refs/remotes`, `refs/tags` y
  `packed-refs`; sin proceso `git` por pulsación.
- Sin archivos de índice ni escritura en los historiales del shell.

Si el prompt usa un formato no reconocido (`$ `, `% `, `# `, `❯`, `➜`, `λ`,
`»`, `→`, `> `) o el comando ocupa varias líneas, no se muestran sugerencias.
En fish se omite el historial inline porque fish ya trae autosugerencias.

---

## Sidebar, sesiones y splits

La sidebar tiene dos modos:

- **Sesiones**: árbol de sesiones/workspaces y sus panes.
- **Archivos**: explorador del cwd del pane enfocado.

### Árbol de sesiones

Cada sesión es un workspace con uno o más panes:

- Una sesión con un solo pane ocupa una fila.
- Cuando la sesión tiene varios panes, aparece una fila padre y los panes como
  hijos, con conectores `├`/`└`.
- El padre muestra un chevron `▾`/`▸` para plegar o desplegar sus hijos y un
  contador de panes.
- El título inicial de una sesión es **Nueva sesión**. Cuando el shell envía su
  título por OSC, el padre se actualiza.
- Los títulos que son solo una ruta (por ejemplo, el directorio actual que
  reporta ConPTY/cmd.exe en Windows) se ignoran para no ensuciar la sidebar.
- Los hijos muestran el título del pane o, si no hay, el shell (`zsh`, `bash`,
  `PowerShell`…).
- Clic en el padre: enfoca el pane activo de esa sesión.
- Clic en un hijo: enfoca ese pane concreto.
- `×` en el padre cierra la sesión completa.
- `×` en un hijo cierra solo ese pane.
- El pane enfocado se resalta con una barra de acento.
- Arrastra una sesión desde su fila para reordenarla; una línea de acento marca
  la posición de destino y el orden se guarda con la sesión.

Acciones:

- `+` crea una nueva sesión con el shell por defecto.
- `▾` abre el selector de perfiles; solo aparece en Sesiones.
- El botón **E/T** de la cabecera alterna Archivos/Sesiones; globalmente usa
  `Ctrl+Shift+E` / `Cmd+Shift+E`.
- `«` colapsa la sidebar; el rail `»` la recupera.
- Rueda del ratón para hacer scroll si hay muchas sesiones o panes.

Dentro de cada sesión:

- `Cmd+D` / `Ctrl+Shift+D`: split a la derecha.
- `Cmd+Shift+D` / `Ctrl+Shift+Down`: split abajo.
- Los nuevos panes aparecen como hijos del árbol y expanden la sesión.
- Los divisores se arrastran con el ratón.
- El pane enfocado muestra un borde de acento.
- `Cmd+Alt+flechas` / `Ctrl+Alt+flechas`: mover el foco.
- `Cmd+\`` / `Ctrl+Shift+\``: ciclar panes.

### Restauración de sesión

Al cerrar F-Terminal se guarda automáticamente el estado de la ventana principal
y se recupera en el siguiente arranque:

- Pestañas abiertas, su orden y las pestañas colapsadas.
- Splits con su dirección y proporción, y el pane enfocado.
- Perfil de shell y directorio de trabajo de cada pane.
- Tamaño, posición y estado maximizado de la ventana.
- Scrollback de cada terminal (texto y colores, sin secuencias OSC).

Los procesos que estaban corriendo no se recuperan: se lanzan shells nuevos en
el directorio guardado. El historial de comandos sigue viviendo en el shell.

El guardado es automático y no bloquea la UI: los metadatos se escriben como
mucho cada 2 segundos y el scrollback se exporta en un barrido repartido entre
frames (un pane por tick) cada 30 segundos, además de un guardado final al
cerrar. La serialización y la escritura se hacen en un hilo aparte. Al abrir una
pestaña, el historial de sus panes se lee en paralelo.
Los datos viven junto a la configuración:

| Plataforma | Ruta |
|---|---|
| macOS / Linux | `~/.config/f-terminal/session.json` |
| Windows | `%APPDATA%\f-terminal\session.json` |

El scrollback se guarda en `session-scrollback/*.ansi` y puede contener
información sensible que haya aparecido en la terminal.

Para desactivarlo, en `config.toml`:

```toml
[session]
restore = false
```

Si quieres restaurar pestañas y directorios pero no el contenido anterior,
mantén `restore = true` y pon `restore_scrollback = false`.

---

## Explorador y editor

### Explorador

- Sigue el cwd real del shell enfocado.
- Carpetas expandibles con `▸`/`▾`.
- Archivos abribles con un clic.
- Botón `←` para subir al directorio padre. La navegación manual se mantiene
  hasta que cambie el cwd del shell o el pane enfocado.
- Botón `↻` para refrescar.
- No usa file watching: se actualiza al hacer `cd` o al refrescar.

### Editor

- Panel a la derecha de la terminal.
- Divisor arrastrable para redimensionar.
- `Cmd+S` / `Ctrl+S` guarda.
- `Esc` o `Cmd+W` cierra. El punto de acento indica cambios sin guardar.
- Clic con el ratón coloca el cursor en la fila y columna pulsadas.
- Resaltado TextMate con `syntect` en un hilo aparte, con debounce de 400 ms y un
  único trabajo en vuelo para no encolar ficheros completos mientras escribes.
- `.jsx`, `.mjs`, `.cjs`, `.ts` y `.tsx` usan la gramática de JavaScript como
  fallback; `.jsonc` y `.json5` usan la de JSON. TOML todavía no tiene gramática
  en el paquete por defecto y se muestra sin colores.
- Los buffers de texto se cachean por pane; al escribir solo se reescala la fila
  editada y los movimientos de cursor no reescalan texto.

Limitaciones actuales del editor:

- Sin undo/redo ni selección de texto.
- Sin file watching ni autoguardado.
- Sin resaltado en vivo para archivos grandes: más de 64 KB, más de 1.000
  líneas o líneas de más de 2 KB.
- Solo UTF-8 y archivos de hasta 2 MB.
- Los tabs se expanden a 4 espacios al cargar.
- Cerrar el editor descarta cambios no guardados.

---

## Footer Git por terminal

Cada pane muestra dos burbujas abajo a la izquierda en una barra compacta de
una sola línea:

1. Rama actual, con un punto de acento.
2. Cambios del working tree: `+N -N`, en una sola píldora con la mitad verde
   para adiciones y la mitad roja para eliminaciones.

Detalles:

- Las burbujas tienen icono vectorial (rama y archivo de diff), borde y un
  pequeño margen inferior.
- Clic en la burbuja de la rama: abre un popup con las ramas locales. Puedes
  usar `↑↓`, `Home`/`End` y `Enter`, o hacer clic en una rama para hacer
  checkout. `Esc` o un clic fuera lo cierran.
- Clic en la burbuja de cambios: abre una vista dividida a la derecha con el
  diff completo del working tree, con cabeceras por archivo, números de línea y
  los colores de adición/eliminación que usa Code Review. `R` recarga, `Esc` o
  `×` la cierran, la rueda/`↑↓`/`PgUp`/`PgDn` hacen scroll y `Shift`+rueda o
  `←`/`→` desplazan horizontalmente.
- El panel de la terminal se pinta con el fondo real del TUI (incluidos los
  colores SGR de opencode), así que el footer y el espacio sobrante no muestran
  una franja del color de la app.
- El footer tiene una altura compacta y aprovecha el espacio sobrante del grid
  para mantener la tipografía legible.
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

### Pestaña Locales

- **Cambios locales**: cambios del árbol de trabajo, incluidos untracked.
- **Rama contra main/master**: diff de la rama actual contra la base.
- **Últimos 15 commits**.
- `↑↓` para mover, `Enter` o clic para abrir, `R` para refrescar.

### Pestaña Pull requests / Merge requests

- Detecta el proveedor a partir del remoto `origin`:
  - **GitHub**: la pestaña se llama **Pull requests** y usa `#123`.
  - **GitLab**: la pestaña se llama **Merge requests** y usa `!123`.
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

Se abre con `Cmd+Shift+M` / `Ctrl+Shift+M` o desde el menú de macOS.

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

## Agent Mode

Se abre con `Cmd+Shift+A` / `Ctrl+Shift+A` o desde el menú de macOS. La ventana
y el menú usan el nombre **Agentes**; Agent Mode es el nombre de la función. Es
una **ventana independiente** que puede estar abierta mientras usas la terminal,
Code Review o el gestor Git.

### Centro de Agentes

La ventana funciona como un dashboard de supervisión:

- **Cabecera**: agentes activos, pendientes de atención, **+ Lanzar**, **Vistos**,
  **Actualizar** y cierre.
- **KPIs**: activos/bloqueados, atención pendiente, CPU y RAM agregadas, y diff
  Git acumulado por repositorio.
- **Buscador**: filtra por nombre, estado, mensaje, título, pestaña, rama,
  comando, ruta o PID. Los contadores de los filtros respetan la búsqueda.
- **Filtros**: Todos, Atención, Activos y Terminados; filtro de origen
  (internos/externos) y orden por prioridad, CPU, RAM, tiempo o nombre.
- **Lista**: cada fila muestra estado, mensaje o última salida, repositorio,
  rama con `+N/-N`, subagentes/procesos hijos, uptime, pane y telemetría.
- **Inspector**: pestañas **Resumen**, **Consola** y **Subagentes**. La consola
  conserva hasta 60 líneas de la pantalla visible y se desplaza con la rueda
  del ratón.
- **Acciones**: enfocar el pane, abrir Git o Code Review en su directorio,
  copiar ruta/PID, marcar como visto y matar el proceso con confirmación.
- **Respuesta rápida**: `I` abre el campo de mensaje; `Enter` lo envía al pane.
  `Y` aprueba si el agente está bloqueado. También hay botones para aprobar,
  enviar `Enter`, mandar `Ctrl+C` y pegar desde el portapapeles.
- **Lanzador**: `N` abre la barra de perfiles; `1` Claude, `2` Codex,
  `3` OpenCode y `4` Junie. Lanza el CLI en una pestaña nueva usando el
  directorio del agente seleccionado (o el cwd enfocado).

Muestra todos los agentes detectados, tanto los que corren dentro de
F-Terminal como los que corren en otras terminales o IDEs (estos últimos en
modo lectura). La lista se ordena por prioridad:

1. Permisos o preguntas pendientes.
2. Avisos de atención.
3. Errores.
4. Trabajo terminado sin leer.
5. Trabajando.
6. Inactivos.
7. Procesos externos.

Estados:

- **Trabajando**: el agente está generando o ejecutando herramientas.
- **Pide permiso / Pide respuesta / Pide login**: el agente está bloqueado.
- **Requiere atención**: notificación o campana sin estado estructurado.
- **Terminado / Error**: el turno terminó; el aviso desaparece al enfocar el pane.
- **Inactivo**: el agente sigue vivo, esperando el siguiente prompt.
- **En ejecución** (externo): proceso detectado, sin estado en tiempo real.

### Detección

La fuente principal es **OSC 7501** (Program Status Protocol). F-Terminal
responde a la consulta de soporte y lee los informes que escriben los agentes:

| Agente | Estado preciso |
|---|---|
| Claude Code | `OSC 7501` desde 2.1.295 |
| Codex CLI | `OSC 7501` desde 0.163; `OSC 9` para completados |
| OpenCode | OpenCode 2.x; versiones anteriores usan el fallback |
| Junie | Hooks opcionales (ver abajo); si no, proceso + actividad + campana |
| Otros | Cualquier agente que implemente `OSC 7501` |

Además hay un watcher de procesos en background que recorre el árbol de cada
pane y reconoce `claude`, `codex`, `opencode` y `junie`. Ese watcher sirve para
saber si el agente sigue vivo, para los agentes sin protocolo y para listar los
procesos externos. El escaneo corre cada 2 segundos por defecto, en un hilo
aparte, y no bloquea la UI.

### Controles

| Acción | Tecla |
|---|---|
| Mover selección | `↑` / `↓`, `J` / `K`, `PgUp` / `PgDn`, `Home` / `End` |
| Enfocar pane del agente | `Enter` o doble clic |
| Abrir mensaje rápido | `I` |
| Aprobar prompt y/n | `Y` o botón **Aprobar** |
| Enviar `Enter` / `Ctrl+C` | botones de la fila rápida |
| Pegar en búsqueda o mensaje | `Ctrl/Cmd+V`, `Shift+Insert` |
| Buscar | `/` o `Cmd/Ctrl+F`; `↑` / `↓` mueven la selección; `Esc` limpia |
| Cambiar filtro de estado | `Tab` / `Shift+Tab`, `F`, `1`–`4` |
| Cambiar origen | `E` |
| Cambiar orden | `S` |
| Pestañas del inspector | `[` / `]` |
| Abrir lanzador | `N`; `1`–`4` eligen perfil |
| Refrescar procesos | `R` o `F5`, o **Actualizar** |
| Abrir Git / Code Review | `G` / `V` |
| Copiar ruta o PID | `C` |
| Marcar visto / todos vistos | `M` / `A` |
| Matar proceso | `X`, `Supr` o `Backspace`; doble pulsación o **Matar PID** |
| Cerrar ventana | `Esc`, `Cmd/Ctrl+W` o `×` |

La primera pulsación de `X`/`Supr` arma el cierre y la segunda confirma. Al
matar un agente se envía primero `SIGTERM`; si sigue vivo después de dos
segundos, se fuerza `SIGKILL` (en Windows, `taskkill /F`). El aviso del resultado
aparece en el pie de la ventana.

Los agentes externos aparecen como `externo` y no se pueden enfocar ni
responder porque su PTY pertenece a otra aplicación. Sí se pueden inspeccionar
y terminar desde Agent Mode si tienen un PID conocido.

### Junie con hooks

Junie todavía no implementa `OSC 7501`, pero sus hooks permiten obtener estados
precisos. F-Terminal lee un log pequeño en
`~/.f-terminal/junie-events.log`; el pane se identifica con la variable
`F_TERMINAL_PANE`, que F-Terminal inyecta en cada shell.

En macOS/Linux, añade esto a `~/.junie/config.json` (solo la parte `hooks`):

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|working|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|done|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ],
    "StopFailure": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|error|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ],
    "SessionEnd": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|end|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ]
  }
}
```

En Windows, el comando equivalente usa PowerShell, porque `cmd` complica el
escape de las barras verticales:

```text
powershell -NoProfile -Command "Add-Content -Path \"$env:USERPROFILE\\.f-terminal\\junie-events.log\" -Value \"$env:F_TERMINAL_PANE|working|\""
```

Cambia `working` por `done`, `error` o `end` en cada hook.

No configures un hook `PermissionRequest` solo para registrar el estado: en
Junie, un hook de permiso que termina con éxito sin decisión **autoaprueba** la
acción. Los permisos pendientes se muestran como **Requiere atención** mediante
la campana, el título y la actividad del terminal.

### Configuración

```toml
[agents]
# Descubre agentes y responde a OSC 7501.
enabled = true
# Incluye agentes de otras terminales/IDEs (solo lectura).
show_external = true
# Intervalo del escaneo de procesos, en milisegundos.
poll_interval_ms = 2000
```

### Limitaciones

- El estado preciso depende de que el agente implemente `OSC 7501` o de los
  hooks de Junie.
- `tmux` y `screen` pueden bloquear `OSC 7501`; el watcher de procesos sigue
  funcionando, pero el estado puede ser aproximado.
- En SSH el proceso remoto no es visible, aunque los informes `OSC` pueden
  llegar por el PTY.
- En WSL los procesos Linux no son visibles desde `sysinfo` de Windows; los
  informes `OSC` sí pueden atravesar el PTY.
- Los agentes externos no tienen estado en tiempo real; se pueden inspeccionar
  (telemetría, comando, PID) y terminar, pero no controlar porque su PTY
  pertenece a otra aplicación.
- No hay aprobación estructurada: `Y`, `I` y los botones envían pulsaciones al
  PTY; la decisión final la toma el CLI del agente.

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
| Cambiar Sesiones / Archivos | `Cmd+Shift+E` |
| Buscar en el scrollback | `Cmd+Shift+F` |
| Abrir enlace bajo el cursor | `Cmd+Click` |
| Abrir Code Review | `Cmd+Shift+R` |
| Abrir gestor Git | `Cmd+Shift+M` |
| Abrir Agentes | `Cmd+Shift+A` |
| Abrir ajustes (`config.toml`) | `Cmd+Shift+S` |
| Abrir explorador del sistema | `Cmd+Shift+O` |
| Guardar en el editor | `Cmd+S` |
| Cerrar editor | `Esc` o `Cmd+W` |
| Copiar / pegar | `Cmd+C` / `Cmd+V` |
| Limpiar terminal | `Cmd+K` |
| Aumentar / reducir / restablecer letra | `Cmd+=` / `Cmd+-` / `Cmd+0` |

### Globales — Linux / Windows

| Acción | Atajo |
|---|---|
| Nueva terminal (perfil por defecto) | `Ctrl+Shift+T` |
| Elegir perfil de shell | `Ctrl+Shift+N` |
| Cerrar pane | `Ctrl+Shift+W` |
| Split derecha / abajo | `Ctrl+Shift+D` / `Ctrl+Shift+Down` |
| Cambiar de terminal | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+Shift+[ / ]` |
| Mover foco entre panes | `Ctrl+Alt+flechas`, `Ctrl+Shift+\`` |
| Mostrar/ocultar sidebar | `Ctrl+Shift+B` |
| Cambiar Sesiones / Archivos | `Ctrl+Shift+E` |
| Buscar en el scrollback | `Ctrl+Shift+F` |
| Abrir enlace bajo el cursor | `Ctrl+Click` |
| Abrir Code Review | `Ctrl+Shift+R` |
| Abrir gestor Git | `Ctrl+Shift+M` |
| Abrir Agentes | `Ctrl+Shift+A` |
| Abrir ajustes (`config.toml`) | `Ctrl+Shift+S` |
| Abrir explorador del sistema | `Ctrl+Shift+O` |
| Guardar en el editor | `Ctrl+S` |
| Cerrar editor | `Esc` o `Ctrl+W` |
| Copiar / pegar | `Ctrl+Shift+C` / `Ctrl+Shift+V`, `Ctrl+Insert` / `Shift+Insert` |
| Limpiar terminal | `Ctrl+Shift+K` |
| Aumentar / reducir / restablecer letra | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |

El tamaño de letra del terminal se puede cambiar en caliente con esos atajos.
El valor se guarda automáticamente en `font_size` de `config.toml`; `0`
restablece el valor por defecto (14 pt).

`Shift+Enter` envía un linefeed (`LF`) en el terminal, que es lo que usan
OpenCode, Claude Code y Codex para insertar un salto de línea en el prompt.
`Ctrl+J` y `Alt+Enter` también funcionan como alternativas.

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

### Agent Mode

| Acción | Tecla |
|---|---|
| Mover selección | `↑` / `↓`, `J` / `K`, `PgUp` / `PgDn`, `Home` / `End` |
| Enfocar pane del agente | `Enter` o doble clic |
| Mensaje rápido / aprobar | `I` / `Y` |
| Buscar | `/` o `Cmd/Ctrl+F` |
| Cambiar filtro / origen / orden | `Tab`, `F`, `1`–`4` / `E` / `S` |
| Pestañas del inspector | `[` / `]` |
| Lanzador de agentes | `N`; `1`–`4` |
| Refrescar procesos | `R` o `F5` |
| Matar proceso (doble pulsación) | `X`, `Supr` o `Backspace` |
| Cerrar ventana | `Esc`, `Cmd/Ctrl+W` o `×` |

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
- Agentes: watcher de procesos en un hilo; `OSC 7501` se parsea en el hilo de UI sin trabajo extra por frame.
- Los resultados llevan `request_id` para descartar respuestas obsoletas.
- Las operaciones mutantes de Git se serializan con un estado `busy`.
- Los tokens de GitHub y GitLab se pasan a `curl` por stdin.

---

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

---

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

---

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

---

## Contribuir

F-Terminal está en desarrollo activo y las contribuciones son bienvenidas:
bugs, mejoras, documentación, pruebas en Linux/Windows o nuevas ideas.

1. Lee [CONTRIBUTING.md](CONTRIBUTING.md).
2. Revisa el [roadmap](#roadmap) y los issues abiertos.
3. Abre un issue con la plantilla correspondiente antes de un cambio grande.
4. Ejecuta `cargo fmt`, `cargo clippy` y `cargo test` antes del pull request.

Al participar aceptas el [Código de Conducta](CODE_OF_CONDUCT.md). Las
contribuciones se publican bajo AGPL-3.0.

---

## Seguridad

No publiques vulnerabilidades en un issue público. Usa el reporte privado:

<https://github.com/fiopans1/F-Terminal/security/advisories/new>

Consulta [SECURITY.md](SECURITY.md) para versiones soportadas, alcance y
tiempos de respuesta.

---

## Changelog

Los cambios relevantes se documentan en [CHANGELOG.md](CHANGELOG.md).

---

## Licencia

F-Terminal se distribuye bajo la **GNU Affero General Public License v3.0**,
la misma licencia que
[`task-manager`](https://github.com/fiopans1/task-manager). Consulta
[`LICENSE`](LICENSE) para el texto completo.

Copyright © 2026 fiopans1.
