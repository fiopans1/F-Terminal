# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí. El formato
sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto
usa [Versionado Semántico](https://semver.org/lang/es/).

## Unreleased

### Added

- Diff del working tree embebido rediseñado: cada archivo es un bloque con
  cabecera destacada, regla separadora y acción `⟲ revert` por archivo. El
  segundo clic (o `Enter`) confirma, `Esc` cancela, los untracked se eliminan y
  el resto vuelve a `HEAD`. Code Review añade el mismo revert en **Cambios
  locales** con botón, `Supr` o `Backspace`.
- `Ctrl+Shift+X` / `Cmd+Shift+X` cierra la sesión completa (todos sus panes),
  incluso con el editor enfocado y sin diálogo de confirmación; `Ctrl+Shift+W`
  cierra el pane enfocado y, si es el último, la sesión. En macOS
  `Cmd+Shift+W` sigue funcionando como alias.
- Búsqueda en el scrollback del pane enfocado con `Cmd+Shift+F` (macOS) o
  `Ctrl+Shift+F` (Linux/Windows): texto literal con *smart case*, resaltado de
  todas las coincidencias, contador, navegación con `Enter`/`Shift+Enter` y
  salto automático al resultado. Límite de 2000 coincidencias para no penalizar
  la interfaz.
- Enlaces clicables: se detectan hipervínculos `OSC 8` y URLs planas
  (`http://`, `https://`, `www.`, `mailto:`), se subrayan al pasar el ratón y se
  abren con `Ctrl+Click` / `Cmd+Click` en el navegador del sistema. Los esquemas
  no permitidos (`file://`, etc.) se ignoran.
- Reordenación de sesiones arrastrando su fila en la sidebar, con línea de
  inserción, pane enfocado preservado y orden persistido en la sesión guardada.
- Sesiones fijadas desde el menú contextual (clic derecho → **Pin tab** /
  **Unpin tab**): quedan agrupadas arriba, el arrastre no las mezcla con las no
  fijadas y el estado se guarda con la sesión.
- Renombrado en línea desde el menú contextual (**Rename**) o con `F2`: `Enter`
  confirma, `Esc` cancela y un título vacío recupera el nombre automático. El
  nombre personalizado se guarda y tiene prioridad sobre el título que reporta
  el shell.
- Barra de título propia en Windows y Linux: icono de la aplicación, título de
  la sesión activa, controles de minimizar/maximizar/cerrar, arrastre, doble
  clic para maximizar y redimensionado desde los bordes. En macOS, una única
  barra superior de altura nativa integra los semáforos del sistema, el icono,
  **F-Terminal** y el botón **?**; Sessions queda como cabecera de la sidebar y
  la terminal empieza justo debajo de la barra.
- Botón **?** en la barra de título con un desplegable About: desarrollador,
  versión actual, enlace al repositorio y una fila reservada para el futuro
  botón de actualizaciones. En macOS, About también aparece en el menú nativo.
- Recarga en caliente de `config.toml`: al guardar el fichero se aplican sin
  reiniciar el tema, tipografías, márgenes, scrollback, shell por defecto,
  agentes y opciones de comportamiento. `font_family` y `render.mode` siguen
  requiriendo reinicio. Si el fichero está a medio escribir, se conserva la
  configuración anterior.
- Restauración de sesión al reabrir F-Terminal: pestañas, splits, perfiles,
  directorios, geometría de la ventana y scrollback con atributos de texto. El
  guardado se hace en segundo plano, la carga del historial en paralelo y la
  exportación se reparte entre frames. Puede desactivarse con
  `[session] restore = false`.
- Autocompletado inline y popup: historial, comandos del `PATH`, subcomandos de
  Git, rutas y ramas/tags, con cachés acotadas y sin procesos por pulsación.
- Agent Mode: ventana independiente con agentes detectados dentro y fuera de
  F-Terminal, estados en tiempo real vía `OSC 7501`, watcher de procesos y
  soporte opcional de hooks de Junie.
- Agent Mode permite forzar un escaneo inmediato con `R`/`F5` o el botón
  **Actualizar**, y terminar el proceso seleccionado con `X`/`Supr` o
  **Matar PID** (confirmación en dos pasos; `SIGTERM` y después `SIGKILL` si es
  necesario).
- Agent Mode se convierte en un **Centro de Agentes**: KPIs de activos,
  atención, CPU/RAM y diff Git; buscador por nombre, rama, ruta, PID o comando;
  filtros de estado y origen; orden por prioridad, CPU, RAM, tiempo o nombre;
  inspector con resumen, consola desplazable (hasta 60 líneas visibles) y
  subagentes; mensajes rápidos con `I`, aprobación con `Y`, botones
  `Enter`/`Ctrl+C`, copiar ruta o PID, marcar visto y lanzador de
  Claude/Codex/OpenCode/Junie en una pestaña nueva.
- Zoom del terminal con `Cmd/Ctrl+=`, `Cmd/Ctrl+-` y `Cmd/Ctrl+0`; el valor se
  guarda en `font_size` y se aplica sin reiniciar.
- `Shift+Enter` envía `LF` para insertar saltos de línea en agentes como
  OpenCode, Claude Code y Codex; `Ctrl+Insert`/`Shift+Insert` copian y pegan.
- Rediseño de las ventanas Git, Code Review y Agentes: carcasa común, escala
  tipográfica legible, botones y foco consistentes, scrollbars, estados vacíos
  accionables y copy en español. El tema Foundry estrena la nueva identidad.
- Optimización del camino caliente de terminal: frames por pane reutilizados,
  actualización por líneas dañadas, buffers de texto por fila, caché de
  etiquetas/medidas y autocompletado cacheado. Git, Code Review, explorador y
  agentes reducen procesos, polling y clones; Code Review y los snapshots Git
  cargan en segundo plano y los PR/MR se piden de forma lazy.
- El explorador de archivos tiene un botón para subir al directorio padre y
  mantiene la navegación manual hasta que cambie el cwd del pane.
- Nueva sección `[agents]` en `config.toml` (`enabled`, `show_external`,
  `poll_interval_ms`).
- `Cmd+Shift+←/→/↑/↓` / `Ctrl+Shift+←/→/↑/↓` crea splits en la dirección
  indicada; `Cmd+Shift+P` / `Ctrl+Shift+P` abre `config.toml` en el editor
  integrado y el elemento de menú «Abrir configuración…» hace lo mismo.
- Temas: Foundry, Catppuccin Mocha, Dracula, Nord, Gruvbox Dark, One Dark,
  Solarized Dark, GitHub Dark y Kanagawa, además de Tokyo Night.
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

- Los splits se crean ahora con las flechas en las cuatro direcciones:
  `Cmd+Shift+←/→/↑/↓` en macOS y `Ctrl+Shift+←/→/↑/↓` en Linux/Windows. Se
  eliminan `Cmd+D`, `Cmd+Shift+D/S` y `Ctrl+Shift+D/S`, que chocaban con
  atajos globales como el guardado de repetición instantánea de AMD Adrenalin
  en Windows.
- El arranque ya no espera a los shells: los panes se construyen en segundo
  plano (con un máximo de cuatro a la vez), la ventana pinta placeholders
  inmediatamente y el PTY empieza a leerse al insertar el pane, sin perder
  salida temprana. Las pulsaciones escritas durante ese intervalo se guardan y
  se envían al shell en cuanto está listo.
- Al restaurar la sesión, los panes de la pestaña activa se prearrancan con el
  grid guardado mientras se inicializan la GPU y las fuentes; si el grid real
  cambia, se redimensionan antes de empezar a leer. Cambiar a una pestaña
  restaurada o crear un split ya no bloquea la UI.
- El índice de fuentes del sistema se guarda en caché (solo metadatos, con
  validación por tamaño/mtime y vuelta al escaneo completo ante cualquier
  problema), lo que reduce el arranque en caliente sin cambiar la tipografía ni
  el fallback.
- La interfaz está ahora íntegramente en inglés: menús, sidebar, ventanas Git,
  Code Review y Agentes, estados, atajos y mensajes de estado.
- Las ventanas Git, Code Review y Agentes ya no dibujan una “×” de cierre
  propia; se cierran con el botón nativo de la ventana.
- El terminal y la UI usan shaping avanzado con fallback para emojis, símbolos
  y otros caracteres que no están en la fuente principal.
- `Ctrl+Shift+F` / `Cmd+Shift+F` abre la búsqueda en el scrollback; la sidebar
  cambia entre Sesiones y Archivos con `Ctrl+Shift+E` / `Cmd+Shift+E`. Los
  splits usan `Cmd+Shift+flechas` / `Ctrl+Shift+flechas`.
- Se eliminó la firma Authenticode del instalador de Windows: las releases se
  publican sin firmar y ya no hace falta configurar ningún certificado ni
  secreto de firma.
- Foundry es ahora el tema por defecto: grafito frío con acento de latón, una
  identidad propia para F-Terminal; la plantilla documenta todas las paletas.
- El gestor Git usa `Cmd+Shift+M` / `Ctrl+Shift+M`; `Ctrl+Shift+G` queda libre
  porque Windows lo intercepta para la grabación de la Xbox Game Bar.
- Las burbujas de rama y de `+N/-N` del footer son más grandes, con borde,
  iconos vectoriales, más separación y margen inferior.
- Los botones de cerrar de la sidebar y del panel lateral se dibujan como icono
  vectorial, no como glifo de fuente, para que se vean igual en Windows.
- Los tests de render GPU se omiten en CI porque requieren un adaptador real.
- El README queda como resumen del proyecto: la documentación completa se
  traslada a `docs/`, con versiones en español e inglés.

### Fixed

- El primer frame revalida la escala de la ventana después de mostrarse, para
  que macOS no estire un frame a 1x en pantallas Retina durante el arranque.
- Clippy en Windows: la detección de fuentes ya no declara `HOME` como variable
  no usada fuera de Unix.
- Al restaurar la sesión, el scrollback ya no guarda las filas vacías de debajo
  del cursor ni la línea viva del prompt: el prompt reaparece donde estaba en
  lugar de acumular líneas en blanco y saltar al fondo en cada reinicio.
- `show_user_in_prompt = false` sigue ocultando `user@host` en panes estrechos:
  cuando el prompt se parte en varias filas, el prefijo se recorta en todas
  ellas en vez de reaparecer al abrir más terminales.
- El cursor del editor se coloca ahora al hacer clic y el frame cacheado se
  invalida, por lo que el cursor se mueve de inmediato.
- La respuesta del editor al teclear ya no se bloquea con el resaltado de
  sintaxis: se agrupa un único trabajo en vuelo, hay más debounce, solo se
  reescala la fila editada y el resaltado se omite en archivos grandes.
- Al abrir una terminal nueva se muestra **Iniciando …** mientras el shell
  arranca, en lugar de un panel en blanco.
- Al abrir un perfil WSL desde el selector en el home de Windows, el `mise` de
  Linux ya no intenta cargar el config global de Windows y la terminal arranca
  sin el error de configuración no trusted.
- El instalador de Windows vuelve a generarse en los tags de release; el job ya
  no aborta cuando no hay un certificado de firma configurado.
- Los títulos de ventana que solo contienen una ruta (ConPTY/cmd.exe en
  Windows) se ignoran; la sesión vuelve a mostrar **Nueva sesión** en vez del
  directorio actual.
- El watcher de procesos de Agent Mode se reinicia si no responde en 8 segundos
  o si su hilo terminó, en lugar de quedarse sin actualizar la lista.
- En Windows, los prompts de PowerShell, cmd y Git Bash/MSYS2/Cygwin informan
  la ruta por `OSC 9;9`. PowerShell no actualiza el cwd del proceso con
  `Set-Location`, así que sin ese informe los splits, las pestañas nuevas, el
  explorador y el footer Git se quedaban en la ruta inicial.
- El footer Git usa una ruta de búsqueda de `git` compatible con instalaciones
  de Git for Windows, Scoop y Chocolatey que no están en el `PATH` de la GUI.
- Las ventanas de Code Review, Git y Agent Mode se pintan una vez ocultas antes
  de mostrarse, evitando el destello blanco inicial en Windows.

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
