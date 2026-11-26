# Configuración

> [Documentación](../README.md) · [English](../en/configuration.md)

F-Terminal usa un único fichero `config.toml`. Se crea automáticamente en el
primer arranque con una plantilla comentada y permisos `600` en Unix.

| Plataforma | Ruta |
|---|---|
| macOS / Linux | `~/.config/f-terminal/config.toml` |
| Linux con `XDG_CONFIG_HOME` | `$XDG_CONFIG_HOME/f-terminal/config.toml` |
| Windows | `%APPDATA%\f-terminal\config.toml` |

La variable `F_TERMINAL_CONFIG` permite usar otra ruta. El índice de fuentes se
guarda en el directorio de caché del sistema (`~/Library/Caches/f-terminal` en
macOS, `$XDG_CACHE_HOME/f-terminal` o `~/.cache/f-terminal` en Linux,
`%LOCALAPPDATA%\f-terminal\cache` en Windows); `F_TERMINAL_CACHE_DIR` permite
cambiarlo.

`Cmd+Shift+P` / `Ctrl+Shift+P` abre el `config.toml` en el editor integrado. Los
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

## Ejemplo completo

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

## Margen izquierdo

`padding_left` añade espacio solo a la izquierda del grid del terminal (8 px por
defecto), para que el texto no quede pegado a la sidebar. El resto de lados
sigue sin marco; `padding` sigue controlando un inset en los cuatro lados si se
quiere.

## Identidad visual

**Foundry** es la identidad propia de F-Terminal: superficies de grafito con un
punto de verde petróleo y un acento cálido de latón. El latón marca la acción
principal, el foco y la rama actual; el verde petróleo queda para el trabajo en
curso y los estados informativos. No es una variante de GitHub Dark: el
contraste cálido/frío busca que la app se reconozca de un vistazo.

La carcasa de las ventanas Git, Code Review y Agentes comparte alturas
(cabecera de 56 px, footer de 28 px), una escala tipográfica mínima de 11 px,
botones de 30 px, foco visible y copy en español. Los estados vacíos explican
qué hacer a continuación y las listas largas muestran scrollbar.

## Temas

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

## Prefijo `user@host`

Con `show_user_in_prompt = false`, F-Terminal oculta el prefijo `user@host`
del prompt visible, incluidas las líneas ya impresas en pantalla, y de los
títulos de pane/ventana (por ejemplo `fiopans1@MacBook-Pro ~/repo %` pasa a
`~/repo %`). Funciona en macOS, Linux y Windows, también cuando el pane es
estrecho y el prompt se parte en varias filas, conserva los colores del resto
del prompt y no modifica tu `PS1`/`PROMPT` ni el historial del shell.

## Scrollback

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

## Render compartido o aislado

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

## Volver al comportamiento anterior

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

## Perfiles de shell

La detección de shells, el selector de perfiles y el campo `[shell] default` se
documentan en [Perfiles de shell](perfiles-de-shell.md).

---

[← Volver al índice](../README.md)
