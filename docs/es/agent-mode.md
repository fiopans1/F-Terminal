# Agent Mode

> [Documentación](../README.md) · [English](../en/agent-mode.md)

Se abre con `Cmd+Shift+A` / `Ctrl+Shift+A` o desde el menú de macOS. La ventana
y el menú usan el nombre **Agentes**; Agent Mode es el nombre de la función. Es
una **ventana independiente** que puede estar abierta mientras usas la terminal,
Code Review o el gestor Git.

## Centro de Agentes

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

## Detección

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

## Controles

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

## Junie con hooks

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

## Configuración

```toml
[agents]
# Descubre agentes y responde a OSC 7501.
enabled = true
# Incluye agentes de otras terminales/IDEs (solo lectura).
show_external = true
# Intervalo del escaneo de procesos, en milisegundos.
poll_interval_ms = 2000
```

## Limitaciones

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

[← Volver al índice](../README.md)
