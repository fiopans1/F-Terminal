# Perfiles de shell

> [Documentación](../README.md) · [English](../en/shell-profiles.md)

F-Terminal detecta shells al arrancar y permite elegir uno al crear una
terminal.

## Detección en macOS y Linux

Orden de detección:

1. `[shell] default` en `config.toml`, si está definido.
2. `$SHELL`.
3. Entradas de `/etc/shells`.
4. Fallbacks `/bin/zsh`, `/bin/bash`, `/bin/sh`.

Los shells conocidos (`zsh`, `bash`, `fish`, `sh`, `dash`, `ksh`, `tcsh`…) se
lanzan con `-l` para que sean login shells y hereden el entorno del usuario.

## Detección en Windows

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

## Selector de shell

En la sidebar, junto al botón `+`, hay un botón `▾`:

- `+` crea una terminal con el perfil por defecto.
- `▾` abre la lista de perfiles dentro de la sidebar.
- `↑↓`/`Home`/`End` mueven la selección, `Enter` crea la terminal y `Esc`
  cierra el selector.
- Un clic sobre un perfil crea la terminal con ese shell.
- Atajo: `Cmd+Shift+N` en macOS, `Ctrl+Shift+N` en Linux/Windows.

Los splits heredan el perfil del pane enfocado. La lista muestra el nombre del
shell y una etiqueta corta (`zsh`, `bash`, `cmd`, `PS`, `pwsh`, `WSL`).

## Shell por defecto

El campo `[shell] default` de `config.toml` permite forzar un shell por ruta o
nombre. Si se omite, se usa la detección automática descrita arriba.

---

[← Volver al índice](../README.md)
