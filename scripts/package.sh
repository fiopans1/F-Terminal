#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
Usage:
  ./scripts/package.sh [macos|linux] [options]

With no platform argument, the script detects the current operating system.

macOS options:
  --arch native|universal   Build only the host architecture or both (default: universal)
  --skip-build              Reuse an existing release binary
  --pkg                     Also generate a .pkg installer
  --sign IDENTITY           Code signing identity (default: ad-hoc)

Linux options:
  --arch amd64|arm64        Package architecture (default: host architecture)
  --skip-build              Reuse an existing release binary
  --no-deb                  Skip the .deb package
  --no-rpm                  Skip the .rpm package

Windows:
  powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
EOF
}

platform="${1:-auto}"
case "$platform" in
    -h|--help)
        usage
        exit 0
        ;;
    macos|linux)
        shift
        ;;
    auto)
        shift
        ;;
    -*)
        platform="auto"
        ;;
    *)
        die "unknown platform '$platform' (expected macos, linux or no argument)"
        ;;
esac

if [[ "$platform" == "auto" ]]; then
    case "$(uname -s)" in
        Darwin)
            platform="macos"
            ;;
        Linux)
            platform="linux"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            die "use scripts/package-windows.ps1 from PowerShell on Windows"
            ;;
        *)
            die "unsupported operating system: $(uname -s)"
            ;;
    esac
fi

case "$platform" in
    macos)
        exec "$REPO_ROOT/scripts/package-macos.sh" "$@"
        ;;
    linux)
        exec "$REPO_ROOT/scripts/package-linux.sh" "$@"
        ;;
esac
