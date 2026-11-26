# Autenticación

> [Documentación](../README.md) · [English](../en/authentication.md)

## Git local y remoto

El gestor Git y el footer usan el binario `git` del sistema. Para operaciones
de red se asume que el usuario ya tiene configurados:

- SSH agent y claves para `git@github.com`.
- O un credential helper para HTTPS.

Los comandos se ejecutan sin `sh -c`, con `GIT_TERMINAL_PROMPT=0`,
`GIT_PAGER=cat` y `LC_ALL=C`. Si Git necesita una contraseña interactiva, la
operación falla y el error se muestra en el footer de la ventana Git.

## GitHub y GitLab para Code Review

La pestaña de cambios remotos usa la API REST del proveedor detectado. Los
repos públicos no necesitan token. Para repos privados, configura un token:

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

### Ejemplos

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

La configuración general, incluidos los tokens, se describe en
[Configuración](configuracion.md).

---

[← Volver al índice](../README.md)
