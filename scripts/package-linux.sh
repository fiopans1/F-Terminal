#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

SKIP_BUILD=0
BUILD_DEB=1
BUILD_RPM=1
REQUESTED_ARCH="native"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch)
            REQUESTED_ARCH="${2:-}"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --no-deb)
            BUILD_DEB=0
            shift
            ;;
        --no-rpm)
            BUILD_RPM=0
            shift
            ;;
        -h|--help)
            printf '%s\n' "Usage: $0 [--arch native|amd64|arm64] [--skip-build] [--no-deb] [--no-rpm]"
            exit 0
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

[[ "$(uname -s)" == "Linux" ]] || die "the Linux installer must be built on Linux"
[[ "$BUILD_DEB" -eq 1 || "$BUILD_RPM" -eq 1 ]] || die "both package formats are disabled"

VERSION="$(app_version)"
validate_version "$VERSION"
DEB_VERSION="$VERSION-1"
DATE="$(utc_date)"

case "$(uname -m)" in
    x86_64|amd64)
        HOST_DEB_ARCH="amd64"
        HOST_RPM_ARCH="x86_64"
        ;;
    aarch64|arm64)
        HOST_DEB_ARCH="arm64"
        HOST_RPM_ARCH="aarch64"
        ;;
    *)
        die "unsupported Linux architecture: $(uname -m)"
        ;;
esac

case "$REQUESTED_ARCH" in
    native)
        DEB_ARCH="$HOST_DEB_ARCH"
        RPM_ARCH="$HOST_RPM_ARCH"
        ;;
    amd64|x86_64)
        DEB_ARCH="amd64"
        RPM_ARCH="x86_64"
        ;;
    arm64|aarch64)
        DEB_ARCH="arm64"
        RPM_ARCH="aarch64"
        ;;
    *)
        die "--arch must be native, amd64 or arm64"
        ;;
esac

[[ "$DEB_ARCH" == "$HOST_DEB_ARCH" ]] \
    || die "cross-architecture packaging is not supported; run this script on the target architecture"

require_command install
require_command ldd

DIST="$REPO_ROOT/dist/linux"
BUILD="$DIST/build"
STAGE="$BUILD/stage"
BIN="$REPO_ROOT/target/release/$BINARY_NAME"
DEB="$DIST/${PACKAGE_NAME}_${DEB_VERSION}_${DEB_ARCH}.deb"

rm -rf "$BUILD"
rm -f "$DIST"/*.deb "$DIST"/*.rpm "$DIST"/SHA256SUMS
mkdir -p "$BUILD" "$DIST"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
    require_command cargo
    log "building $BINARY_NAME for $DEB_ARCH"
    cargo build --locked --release
fi

[[ -f "$BIN" ]] || die "missing release binary: $BIN (run without --skip-build)"

log "staging application files"
install -Dm755 "$BIN" "$STAGE/usr/bin/$BINARY_NAME"
install -Dm644 "$REPO_ROOT/scripts/linux/f-terminal.desktop" \
    "$STAGE/usr/share/applications/f-terminal.desktop"

for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 \
        "$REPO_ROOT/assets/linux/hicolor/${size}x${size}/apps/f-terminal.png" \
        "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps/f-terminal.png"
done

install -d "$STAGE/usr/share/metainfo"
sed \
    -e "s/@VERSION@/$VERSION/g" \
    -e "s/@DATE@/$DATE/g" \
    "$REPO_ROOT/scripts/linux/f-terminal.metainfo.xml" \
    > "$STAGE/usr/share/metainfo/com.fiopans1.f-terminal.metainfo.xml"
chmod 644 "$STAGE/usr/share/metainfo/com.fiopans1.f-terminal.metainfo.xml"

install -Dm644 "$REPO_ROOT/LICENSE" "$STAGE/usr/share/doc/f-terminal/copyright"
install -Dm644 "$REPO_ROOT/README.md" "$STAGE/usr/share/doc/f-terminal/README.md"

GLIBC_VERSION="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}' || true)"
if [[ -n "$GLIBC_VERSION" ]]; then
    LIBC_DEP="libc6 (>= $GLIBC_VERSION)"
else
    LIBC_DEP="libc6"
fi

# Runtime libraries that are loaded dynamically are not always present in
# ldd output, so merge the detected dependencies with a conservative set.
CURATED_DEPS="$LIBC_DEP, libdbus-1-3, libegl1, libfontconfig1, libfreetype6, libgl1, libgles2, libx11-6, libxcb1, libxcursor1, libxi6, libxinerama1, libxkbcommon0, libxkbcommon-x11-0, libxrandr2, libwayland-client0, libwayland-cursor0, libwayland-egl1, libvulkan1"
DETECTED_DEPS=""
if command -v dpkg >/dev/null 2>&1; then
    while IFS= read -r library; do
        package="$(dpkg -S "$library" 2>/dev/null | head -n1 | cut -d: -f1 || true)"
        if [[ -n "$package" && "$package" != "libc6" ]]; then
            DETECTED_DEPS+="$package,"$'\n'
        fi
    done < <(ldd "$BIN" 2>/dev/null | awk '/=> \// { print $3 }' | sort -u)
fi

DEB_DEPENDS="$(
    printf '%s\n%s\n' "$CURATED_DEPS" "$DETECTED_DEPS" \
        | tr ',' '\n' \
        | sed 's/^[[:space:]]*//; s/[[:space:]]*$//' \
        | sed '/^$/d' \
        | sort -u \
        | paste -sd, -
)"

if [[ "$BUILD_DEB" -eq 1 ]]; then
    require_command dpkg-deb
    require_command dpkg
    log "building DEB"
    DEB_ROOT="$BUILD/deb"
    mkdir -p "$DEB_ROOT"
    cp -a "$STAGE/usr" "$DEB_ROOT/"
    mkdir -p "$DEB_ROOT/DEBIAN"

    installed_size="$(du -sk "$STAGE/usr" | awk '{print $1}')"
    cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: $PACKAGE_NAME
Version: $DEB_VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: $MAINTAINER
Installed-Size: $installed_size
Depends: $DEB_DEPENDS
Recommends: curl, git, mesa-vulkan-drivers
Homepage: $HOMEPAGE
Description: Native GPU terminal with Git and code review
 F-Terminal is a native Rust terminal emulator with splits, a file explorer,
 an editor, a Git manager, and GitHub/GitLab code review.
EOF

    cat > "$DEB_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
fi
exit 0
EOF
    cp "$DEB_ROOT/DEBIAN/postinst" "$DEB_ROOT/DEBIAN/postrm"
    chmod 755 "$DEB_ROOT/DEBIAN/postinst" "$DEB_ROOT/DEBIAN/postrm"

    dpkg-deb --build --root-owner-group "$DEB_ROOT" "$DEB" >/dev/null
    dpkg-deb --info "$DEB" >/dev/null
    log "DEB: $DEB"
fi

if [[ "$BUILD_RPM" -eq 1 ]]; then
    require_command rpmbuild
    require_command rpm
    log "building RPM"
    RPMBUILD="$BUILD/rpmbuild"
    mkdir -p "$RPMBUILD"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
    SPEC="$RPMBUILD/SPECS/$PACKAGE_NAME.spec"
    RPM_DATE="$(LC_ALL=C date +"%a %b %d %Y")"

    cat > "$SPEC" <<EOF
Name:           $PACKAGE_NAME
Version:        $VERSION
Release:        1%{?dist}
Summary:        Native GPU terminal with Git and code review
License:        $LICENSE_NAME
URL:            $HOMEPAGE
BuildArch:      $RPM_ARCH
AutoReqProv:    yes
Requires:       libX11
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libwayland-client
Requires:       vulkan-loader
Requires:       mesa-libGL
Requires:       fontconfig
Recommends:     curl
Recommends:     git
Recommends:     mesa-vulkan-drivers

%description
F-Terminal is a native Rust terminal emulator with splits, a file explorer,
an editor, a Git manager, and GitHub/GitLab code review.

%prep

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a %{_stage}/usr %{buildroot}/

%post
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || :
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t /usr/share/icons/hicolor || :
fi

%postun
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || :
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t /usr/share/icons/hicolor || :
fi

%files
%{_bindir}/$BINARY_NAME
%{_datadir}/applications/f-terminal.desktop
%{_datadir}/icons/hicolor/*/apps/f-terminal.png
%{_datadir}/metainfo/com.fiopans1.f-terminal.metainfo.xml
%{_datadir}/doc/f-terminal/copyright
%{_datadir}/doc/f-terminal/README.md

%changelog
* $RPM_DATE fiopans1 <fiopans1@users.noreply.github.com> - $VERSION-1
- Automated release build
EOF

    rpmbuild --quiet -bb \
        --define "_topdir $RPMBUILD" \
        --define "_stage $STAGE" \
        --define "debug_package %{nil}" \
        "$SPEC" >/dev/null

    RPM_FILE="$(find "$RPMBUILD/RPMS" -type f -name '*.rpm' -print -quit)"
    [[ -n "$RPM_FILE" ]] || die "rpmbuild did not produce a package"
    cp "$RPM_FILE" "$DIST/"
    rpm -qip "$DIST/$(basename "$RPM_FILE")" >/dev/null
    log "RPM: $DIST/$(basename "$RPM_FILE")"
fi

(
    cd "$DIST"
    : > SHA256SUMS
    for artifact in *.deb *.rpm; do
        [[ -f "$artifact" ]] || continue
        printf '%s  %s\n' "$(sha256_file "$artifact")" "$artifact" >> SHA256SUMS
    done
)

log "artifacts:"
ls -lh "$DIST"/*.deb "$DIST"/*.rpm 2>/dev/null || true
log "done"
