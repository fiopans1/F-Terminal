# Contribuir a F-Terminal

Gracias por querer mejorar F-Terminal. La documentación está en español, pero
issues y pull requests en inglés son bienvenidos.

## Antes de empezar

- Revisa el [README](README.md), el [roadmap](README.md#roadmap) y los issues
  existentes.
- Para bugs de seguridad, sigue [SECURITY.md](SECURITY.md); no abras un issue
  público.
- Las propuestas grandes conviene discutirlas en un issue antes de escribir
  código. Los cambios pequeños y centrados se revisan mucho más rápido.
- Al participar aceptas el [Código de Conducta](CODE_OF_CONDUCT.md).

## Entorno

Requisitos:

- Rust estable (el proyecto usa `edition = "2024"`; mínimo 1.95).
- Git.
- Un backend gráfico: Metal en macOS, Vulkan/GL en Linux, DX12 en Windows.
- `curl` para las pestañas de PRs/MRs.
- Opcional: `gh` o `glab` autenticados para reutilizar tokens.

Comandos habituales:

```bash
cargo run                 # desarrollo
cargo run --release       # release
RUST_LOG=debug cargo run  # logs detallados
```

La configuración se lee de `~/.config/f-terminal/config.toml` (o
`%APPDATA%\f-terminal\config.toml`). `F_TERMINAL_CONFIG` permite usar otra ruta.

## Estilo

- Sigue el estilo del código existente: Rust 2024, módulos pequeños y errores
  con contexto (`anyhow::Context`).
- Los comentarios y nombres de código van en inglés; la documentación de usuario
  va en español.
- Evita dependencias nuevas si la biblioteca estándar resuelve el problema.
- No mezcles refactors masivos con cambios funcionales.
- Para UI, mantén el render ligero: los datos temporales deben liberarse al
  terminar el frame y las cachés deben tener límites explícitos.

## Tests y calidad

Antes de abrir un pull request:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Los tests `render::text::tests` necesitan un adaptador GPU real. En entornos
headless o CI se omiten con:

```bash
cargo test --all-targets -- --skip render::text::tests
```

La CI ejecuta formato, Clippy y tests en Linux, macOS y Windows. Los
instaladores completos y ConPTY se validan en el workflow de release de tags.

## Commits y pull requests

- Usa [Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
  `fix:`, `docs:`, `chore:`, `refactor:`, `test:`.
- Un commit debe contar una cosa. Si el cambio tiene partes independientes,
  sepáralas.
- Incluye tests para bugs corregidos y para lógica nueva que pueda romperse.
- Actualiza README o CHANGELOG si el cambio es visible para el usuario.
- En el PR explica el problema, la solución y cómo lo has probado, incluyendo
  la plataforma.
- No incluyas credenciales, tokens, rutas privadas ni logs sin revisar.

## Áreas donde ayudar

- Validación real en Linux y Windows: perfiles de shell, ConPTY e instaladores.
- Integración de shell (OSC 133) y completado multilínea.
- Búsqueda en scrollback y render incremental.
- Undo/redo y selección en el editor.
- Temas y pantalla de ajustes.
- Accesibilidad, atajos y documentación.

## Licencia

F-Terminal es software libre bajo **AGPL-3.0-only**. Al enviar una
contribución aceptas que se distribuya bajo esa misma licencia.
