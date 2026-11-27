# Gestor Git

> [Documentación](../README.md) · [English](../en/git-manager.md)

Se abre con `Cmd+Shift+M` / `Ctrl+Shift+M` o desde el menú de macOS.

Es una **ventana nativa independiente**, distinta de Code Review. Puede estar
abierta a la vez que la terminal y que Code Review. Usa el repositorio Git del
cwd del pane enfocado.

## Interfaz

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

## Working tree

- Lista separada de **staged** y **unstaged**.
- Marca de conflicto `U` para archivos sin resolver.
- Clic en `+` para stage, `−` para unstage.
- `Enter`/`Espacio` sobre una fila también hace stage/unstage.
- `Supr`/`Backspace` descarta cambios con confirmación.
- Botones **Stage all** / **Unstage all**.
- Si hay conflictos aparece **Abort merge** (`git merge --abort`).

## Commits

- Campo de mensaje de una línea en la barra inferior.
- **Commit** solo con staged y mensaje no vacío.
- **Amend** activa `git commit --amend`.
- El mensaje se pasa a Git por stdin (`git commit -F -`), sin shell.
- Los commits se hacen en background y muestran el resultado en el footer.

## Ramas, remotos y stash

En la pestaña **REFS**:

- `↑↓` mueve la selección.
- `Enter` hace checkout de una rama local o remota.
- Una rama remota sin local crea una rama de tracking.
- `N` abre el diálogo de nueva rama.
- `M` hace merge de la rama seleccionada.
- `D` borra una rama local con confirmación.
- En stashes, `Pop` aplica y `Drop` elimina con confirmación.
- `R` refresca.

## Remoto

- **Fetch**: `git fetch --all --prune`.
- **Pull**: `git pull --ff-only` para evitar merges inesperados.
- **Push**: `git push`; si no hay upstream, `git push -u origin <rama>`.
- Los comandos de red se ejecutan en background y no bloquean la UI.

## Historial y detalle

- Historial de hasta 300 commits con working tree como primera fila.
- Selección con `↑↓`, `PgUp/PgDn`, `Home/End`.
- Detalle del commit: hash, asunto, autor, fecha y refs.
- Lista de archivos con estado y `+/-`.
- Diff por archivo con numeración y colores.
- `↑↓` cambia de archivo; `PgUp/PgDn` desplaza el diff; `←→` horizontal.

## Refresco

- Working tree cada 2 segundos.
- Historial, refs y detalle tras cada operación o con `R`.
- Todas las operaciones mutantes se serializan: mientras una corre, el resto se
  deshabilita.

## Limitaciones del gestor Git

- Sin `git revert` (commits de reversión), rebase, cherry-pick, gestión de tags
  ni force push. El botón/tecla de descartar del gestor solo revierte cambios
  del working tree.
- Borrar rama usa `-d`; si no está mergeada, Git devuelve error.
- Pull solo fast-forward.
- Conflictos: se muestran y se pueden abortar, pero no hay editor de
  resolución integrado.
- El historial se limita a 300 commits.
- No hay autenticación interactiva: Git debe tener configurados SSH agent o
  credential helper. Se ejecuta con `GIT_TERMINAL_PROMPT=0`.
- No hay push forzado ni gestión de remotos.

---

[← Volver al índice](../README.md)
