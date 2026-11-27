# Code Review

> [Documentación](../README.md) · [English](../en/code-review.md)

Se abre con `Cmd+Shift+R` / `Ctrl+Shift+R` o desde el menú de macOS. Es una
**ventana nativa independiente**: la terminal principal sigue usable.

## Pestaña Locales

- **Cambios locales**: cambios del árbol de trabajo, incluidos untracked.
- **Rama contra main/master**: diff de la rama actual contra la base.
- **Últimos 15 commits**.
- `↑↓` para mover, `Enter` o clic para abrir, `R` para refrescar.

## Pestaña Pull requests / Merge requests

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

## Diff y revisión

- Tres columnas: reviews, selector de archivos y diff.
- Estado por archivo: `M/A/D/R/B`.
- `+adiciones −eliminaciones` por archivo.
- `Espacio` o `M` marca un archivo como revisado.
- En **Cambios locales**, el botón **Revert** (o `Supr`/`Backspace`) revierte
  el archivo seleccionado; el primer clic/pulsación pide confirmación y `Esc`
  cancela. Los untracked se eliminan.
- Las marcas viven solo en memoria durante la sesión.
- `Tab` cambia de panel; `Esc` o `×` cierran.
- El diff se carga en background y se descarta si llega tarde
  (`window_id` + `request_id`).

## Autenticación

Los repos públicos funcionan sin token, con el límite de peticiones de cada
API. Para repos privados, consulta
[Autenticación](autenticacion.md#github-y-gitlab-para-code-review).

---

[← Volver al índice](../README.md)
