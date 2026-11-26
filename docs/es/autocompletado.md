# Autocompletado

> [Documentación](../README.md) · [English](../en/autocomplete.md)

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

La configuración del autocompletado se describe en
[Configuración](configuracion.md#ejemplo-completo).

---

[← Volver al índice](../README.md)
