# Installation and build

> [Documentation](README.md) · [Español](../es/instalacion.md)

F-Terminal is distributed as a native binary and packaged independently for
macOS, Linux and Windows. This page covers requirements, local builds and
installer generation.

## Requirements

- Stable Rust, minimum **1.95**. The project uses `edition = "2024"` and has
  been tested with Rust 1.100.
- An available graphics backend: Metal on macOS, Vulkan/GL on Linux, DX12 on
  Windows.
- Git installed for Code Review, the Git manager and the per-terminal footer.
- `curl` to list and download GitHub PRs or GitLab MRs.
- Optional: authenticated `gh` or `glab` to reuse their tokens.
- On Windows, shells are detected automatically. Git Bash must use
  `...\Git\bin\bash.exe`, not `git-bash.exe`.

## Build and run

```bash
# Development
cargo run

# Optimized release
cargo run --release

# Debug logs
RUST_LOG=debug cargo run
```

The binary is called `f-terminal`.

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```

The app starts in the current directory when launched from a terminal; if it
is opened from Finder, the Start menu or a graphical launcher, it uses the
user's home directory. New terminals inherit the focused pane's cwd, and splits
also inherit its shell profile.

## Installers and releases

F-Terminal is packaged natively for each platform. Artifacts are generated in
`dist/` and **nothing is downloaded during installation**.

### GitHub Actions: tags only

The [`.github/workflows/installers.yml`](../../.github/workflows/installers.yml)
workflow runs only when a `v*` tag is pushed:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Before building, it validates that the tag matches the `version` field in
`Cargo.toml` exactly. It then builds on native runners and publishes a GitHub
Release with:

- `F-Terminal-0.1.0-macos-universal.dmg`
- `F-Terminal-0.1.0-macos-universal.pkg`
- `F-Terminal-Setup-0.1.0-x64.exe`
- `f-terminal_0.1.0-1_amd64.deb`
- `f-terminal-0.1.0-1.x86_64.rpm`
- `SHA256SUMS`

The workflow does not run on every push or through `workflow_dispatch`.

To test the pipeline without publishing a stable version, use a prerelease:

```bash
git tag v0.1.0-alpha-001-test
git push origin v0.1.0-alpha-001-test
```

The workflow validates that `v0.1.0` matches `Cargo.toml`, generates all
installers and publishes the result as a GitHub **prerelease**.

macOS is built on `macos-latest`. If the macOS runner is unavailable (for
example, due to minute limits on a private repository), the release publishes
the Linux/Windows artifacts and leaves a warning; generating the DMG and PKG
requires a macOS runner, an account with minutes or a self-hosted runner.

### Local build

```bash
# macOS
./scripts/package.sh macos

# macOS with a .pkg installer in addition to the .dmg
./scripts/package.sh macos --pkg

# Linux (Debian/Ubuntu or Fedora/RHEL)
./scripts/package.sh linux

# Windows
powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
```

Build requirements:

- macOS: Xcode Command Line Tools, Rust and the `aarch64-apple-darwin` and
  `x86_64-apple-darwin` targets.
- Windows: Rust MSVC, Visual Studio Build Tools and Inno Setup 6.
- Linux: Rust, `dpkg-deb`, `rpmbuild` and `file`.

The macOS script generates a universal `arm64 + x86_64` binary. If no signing
identity is configured, it signs in ad-hoc mode; for public distribution you can
use a Developer ID identity and notarization:

```bash
CODESIGN_IDENTITY="Developer ID Application: ..." \
APPLE_ID="..." APPLE_TEAM_ID="..." APPLE_APP_PASSWORD="..." \
./scripts/package.sh macos
```

The Windows installer **always runs for all users**, with UAC and a target of
`C:\Program Files\F-Terminal`. It is published without an Authenticode
signature, so SmartScreen may show an unknown-publisher warning. Avoiding it
requires an OV/EV certificate issued by a trusted CA.

### User installation

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

Windows installers are published unsigned, so SmartScreen may show its usual
warnings. The `.deb`/`.rpm` packages use the system's base libraries; AppImage
is not included in this iteration.

---

[← Back to the index](README.md)
