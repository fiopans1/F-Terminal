# Sidebar, sesiones, explorador y editor

> [Documentación](../README.md) · [English](../en/interface.md)

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
- Clic derecho en una sesión abre un menú con **Pin tab**/**Unpin tab** y
  **Rename**. Las fijadas se agrupan arriba y no se pueden arrastrar fuera de
  ese grupo; el estado se guarda.
- **Rename** (o `F2` sobre la activa, o doble clic en la fila) edita el título
  en línea: `Enter` confirma, `Esc` cancela y dejarlo vacío recupera el título
  automático. El nombre personalizado se guarda y prevalece sobre el que
  reporta el shell.

Acciones:

- `+` crea una nueva sesión con el shell por defecto.
- `▾` abre el selector de perfiles; solo aparece en Sesiones.
- El botón **E/T** de la cabecera alterna Archivos/Sesiones; globalmente usa
  `Ctrl+Shift+E` / `Cmd+Shift+E`.
- `«` colapsa la sidebar; el rail `»` la recupera.
- Rueda del ratón para hacer scroll si hay muchas sesiones o panes.

Dentro de cada sesión:

- `Cmd+Shift+flechas` / `Ctrl+Shift+flechas`: split a la izquierda, derecha,
  arriba o abajo.
- `Ctrl+Shift+W` cierra el pane enfocado (la sesión si es el último);
  `Ctrl+Shift+X` cierra la sesión entera, también con el editor abierto y sin
  confirmación. En macOS: `Cmd+W`, y `Cmd+Shift+X` o `Cmd+Shift+W` para la
  sesión completa.
- Los nuevos panes aparecen como hijos del árbol y expanden la sesión.
- Los divisores se arrastran con el ratón.
- El pane enfocado muestra un borde de acento.
- `Cmd+Alt+flechas` / `Ctrl+Alt+flechas`: mover el foco.
- <kbd>Cmd+&#96;</kbd> / <kbd>Ctrl+Shift+&#96;</kbd>: ciclar panes.

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
cerrar. La serialización y la escritura se hacen en un hilo aparte.

La restauración tampoco bloquea: los panes de la pestaña activa se prearrancan
con el último grid guardado mientras se inicializan la GPU y las fuentes, y el
resto se crea en segundo plano al abrir su pestaña. La ventana muestra un
placeholder hasta que cada shell está listo; las teclas pulsadas mientras tanto
se conservan. El índice de fuentes se cachea en el directorio de caché del
sistema para que los arranques en caliente no tengan que analizar todas las
fuentes instaladas.

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

[← Volver al índice](../README.md)
