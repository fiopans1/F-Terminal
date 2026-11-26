# Instalación y compilación

> [Documentación](../README.md) · [English](../en/installation.md)

F-Terminal se distribuye como binario nativo y se empaqueta de forma
independiente para macOS, Linux y Windows. Esta página cubre los requisitos, la
compilación local y la generación de instaladores.

## Requisitos

- Rust estable, mínimo **1.95**. El proyecto usa `edition = "2024"` y se ha
  probado con Rust 1.100.
- Un backend gráfico disponible: Metal en macOS, Vulkan/GL en Linux, DX12 en
  Windows.
- Git instalado para Code Review, el gestor Git y el footer por terminal.
- `curl` para listar y descargar PRs de GitHub o MRs de GitLab.
- Opcional: `gh` o `glab` autenticados para reutilizar sus tokens.
- En Windows, los shells se detectan automáticamente. Git Bash debe usar
  `...\Git\bin\bash.exe`, no `git-bash.exe`.

## Compilar y ejecutar

```bash
# Desarrollo
cargo run

# Release optimizado
cargo run --release

# Logs de depuración
RUST_LOG=debug cargo run
```

El binario se llama `f-terminal`.

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```

La app arranca en el directorio actual cuando se lanza desde una terminal; si
se abre desde Finder, el menú Inicio o un lanzador gráfico, usa el directorio
home del usuario. Las terminales nuevas heredan el cwd del pane enfocado y los
splits heredan también su perfil de shell.

## Instaladores y releases

F-Terminal se empaqueta de forma nativa para cada plataforma. Los artefactos
se generan en `dist/` y **no descargan nada durante la instalación**.

### GitHub Actions: solo tags

El workflow [`.github/workflows/installers.yml`](../../.github/workflows/installers.yml)
se ejecuta únicamente cuando se empuja un tag `v*`:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Antes de compilar, valida que el tag coincida exactamente con el campo
`version` de `Cargo.toml`. Después compila en runners nativos y publica un
GitHub Release con:

- `F-Terminal-0.1.0-macos-universal.dmg`
- `F-Terminal-0.1.0-macos-universal.pkg`
- `F-Terminal-Setup-0.1.0-x64.exe`
- `f-terminal_0.1.0-1_amd64.deb`
- `f-terminal-0.1.0-1.x86_64.rpm`
- `SHA256SUMS`

El workflow no se ejecuta en cada push ni mediante `workflow_dispatch`.

Para probar el pipeline sin publicar una versión estable, usa una prerelease:

```bash
git tag v0.1.0-alpha-001-test
git push origin v0.1.0-alpha-001-test
```

El workflow valida que `v0.1.0` coincida con `Cargo.toml`, genera todos los
instaladores y publica el resultado como **prerelease** de GitHub.

macOS se compila en `macos-latest`. Si el runner de macOS no está disponible
(por ejemplo, por límites de minutos en un repositorio privado), la release
publica los artefactos de Linux/Windows y deja un warning; para generar el DMG
y el PKG hace falta un runner macOS, una cuenta con minutos o un runner
self-hosted.

### Build local

```bash
# macOS
./scripts/package.sh macos

# macOS con instalador .pkg además del .dmg
./scripts/package.sh macos --pkg

# Linux (Debian/Ubuntu o Fedora/RHEL)
./scripts/package.sh linux

# Windows
powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
```

Requisitos de build:

- macOS: Xcode Command Line Tools, Rust y los targets
  `aarch64-apple-darwin` y `x86_64-apple-darwin`.
- Windows: Rust MSVC, Visual Studio Build Tools e Inno Setup 6.
- Linux: Rust, `dpkg-deb`, `rpmbuild` y `file`.

El script de macOS genera un binario universal `arm64 + x86_64`. Si no hay
identidad de firma configurada, firma en modo ad-hoc; para distribución pública
se puede usar una identidad Developer ID y notarización:

```bash
CODESIGN_IDENTITY="Developer ID Application: ..." \
APPLE_ID="..." APPLE_TEAM_ID="..." APPLE_APP_PASSWORD="..." \
./scripts/package.sh macos
```

El instalador de Windows se ejecuta **siempre para todos los usuarios**, con
UAC y destino en `C:\Program Files\F-Terminal`. Se publica sin firma
Authenticode, así que SmartScreen puede mostrar un aviso de editor desconocido.
Para evitarlo hace falta un certificado OV/EV emitido por una CA de confianza.

### Instalación para usuarios

```bash
# macOS
open dist/macos/F-Terminal-0.1.0-macos-universal.dmg

# Windows
dist\windows\F-Terminal-Setup-0.1.0-x64.exe

# Debian / Ubuntu
sudo apt install ./dist/linux/f-terminal_0.1.0-1_amd64.deb

# Fedora / RHEL
sudo dnf install ./dist/linux/f-terminal-0.1.0-1.x86_64.rpm
```

Los instaladores de Windows se publican sin firma, así que SmartScreen puede
mostrar sus avisos habituales. Los `.deb`/`.rpm` usan las librerías base del
sistema; no se incluye AppImage en esta iteración.

---

[← Volver al índice](../README.md)
