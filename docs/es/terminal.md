# Terminal

> [Documentación](../README.md) · [English](../en/terminal.md)

Cada pane tiene:

- Su propio PTY y proceso hijo.
- Su propio `Term` de `alacritty_terminal` y su `Frame` renderizable.
- Scrollback independiente.
- Su cwd, que se sigue mediante `sysinfo` cada 750 ms.
- Su estado de selección, cursor y modos VT.
- Un aviso **Iniciando \<perfil\>…** hasta que el shell produce su primera
  salida.

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

## Búsqueda en scrollback y enlaces

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

## Autocompletado

El autocompletado inline y el popup se documentan en
[Autocompletado](autocompletado.md).

---

[← Volver al índice](../README.md)
