#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

ARCH_MODE="universal"
SKIP_BUILD=0
BUILD_PKG=0
SIGN_IDENTITY="${CODESIGN_IDENTITY:--}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch)
            ARCH_MODE="${2:-}"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --pkg)
            BUILD_PKG=1
            shift
            ;;
        --sign)
            SIGN_IDENTITY="${2:-}"
            shift 2
            ;;
        -h|--help)
            printf '%s\n' "Usage: $0 [--arch native|universal] [--skip-build] [--pkg] [--sign IDENTITY]"
            printf '%s\n' "PKG signing uses the PKG_SIGN_IDENTITY environment variable."
            exit 0
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "the macOS installer must be built on macOS"
[[ "$ARCH_MODE" == "native" || "$ARCH_MODE" == "universal" ]] \
    || die "--arch must be native or universal"

VERSION="$(app_version)"
validate_version "$VERSION"
BUNDLE_VERSION="${VERSION%%-*}"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"

if [[ "$ARCH_MODE" == "universal" ]]; then
    TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
    ARCH_LABEL="universal"
else
    TARGETS=("$HOST_TARGET")
    case "$HOST_TARGET" in
        aarch64-apple-darwin) ARCH_LABEL="arm64" ;;
        x86_64-apple-darwin) ARCH_LABEL="x86_64" ;;
        *) ARCH_LABEL="$HOST_TARGET" ;;
    esac
fi

require_command lipo
require_command plutil
require_command codesign
require_command hdiutil
require_command ditto

DIST="$REPO_ROOT/dist/macos"
BUILD="$DIST/build"
APP="$BUILD/$APP_NAME.app"
DMG="$DIST/$APP_NAME-$VERSION-macos-$ARCH_LABEL.dmg"
PKG="$DIST/$APP_NAME-$VERSION-macos-$ARCH_LABEL.pkg"

rm -rf "$BUILD"
rm -f "$DIST"/*.dmg "$DIST"/*.pkg "$DIST"/SHA256SUMS
mkdir -p "$BUILD" "$DIST"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
    require_command cargo
    if [[ "$ARCH_MODE" == "universal" ]]; then
        for target in "${TARGETS[@]}"; do
            if command -v rustup >/dev/null 2>&1; then
                if ! rustup target list --installed | grep -qx "$target"; then
                    die "Rust target $target is not installed; run: rustup target add $target"
                fi
            fi
            log "building $BINARY_NAME for $target"
            MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}" \
                cargo build --locked --release --target "$target"
        done
    else
        log "building $BINARY_NAME for $HOST_TARGET"
        MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}" \
            cargo build --locked --release
    fi
fi

BINARIES=()
if [[ "$ARCH_MODE" == "universal" ]]; then
    for target in "${TARGETS[@]}"; do
        binary="$REPO_ROOT/target/$target/release/$BINARY_NAME"
        [[ -f "$binary" ]] || die "missing release binary: $binary"
        BINARIES+=("$binary")
    done
else
    binary="$REPO_ROOT/target/release/$BINARY_NAME"
    [[ -f "$binary" ]] || die "missing release binary: $binary"
    BINARIES+=("$binary")
fi

log "creating $APP_NAME.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [[ "${#BINARIES[@]}" -eq 1 ]]; then
    cp "${BINARIES[0]}" "$APP/Contents/MacOS/$BINARY_NAME"
else
    lipo -create -output "$APP/Contents/MacOS/$BINARY_NAME" "${BINARIES[@]}"
fi
chmod 755 "$APP/Contents/MacOS/$BINARY_NAME"
cp "$REPO_ROOT/assets/macos/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cp "$REPO_ROOT/LICENSE" "$APP/Contents/Resources/LICENSE"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleExecutable</key>
    <string>$BINARY_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUNDLE_VERSION</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright (C) fiopans1. Licensed under AGPL-3.0.</string>
</dict>
</plist>
EOF

plutil -lint "$APP/Contents/Info.plist" >/dev/null

if [[ "$SIGN_IDENTITY" != "-" ]]; then
    if ! security find-identity -v -p codesigning 2>/dev/null | grep -Fq "$SIGN_IDENTITY"; then
        log "signing identity '$SIGN_IDENTITY' was not found; using ad-hoc signing"
        SIGN_IDENTITY="-"
    fi
fi

log "signing app with identity: $SIGN_IDENTITY"
if [[ "$SIGN_IDENTITY" == "-" ]]; then
    codesign --force --sign - "$APP"
else
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP"
fi
codesign --verify --deep --strict --verbose=2 "$APP"

log "creating DMG"
DMG_ROOT="$BUILD/dmg-root"
mkdir -p "$DMG_ROOT"
ditto "$APP" "$DMG_ROOT/$APP_NAME.app"
ln -s /Applications "$DMG_ROOT/Applications"
cp "$REPO_ROOT/LICENSE" "$DMG_ROOT/LICENSE"
hdiutil create \
    -volname "$APP_NAME $VERSION" \
    -srcfolder "$DMG_ROOT" \
    -ov \
    -format UDZO \
    "$DMG" >/dev/null
hdiutil verify "$DMG" >/dev/null

if [[ "$SIGN_IDENTITY" != "-" ]]; then
    codesign --force --sign "$SIGN_IDENTITY" "$DMG"
    if [[ -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
        log "submitting DMG for notarization"
        xcrun notarytool submit "$DMG" \
            --apple-id "$APPLE_ID" \
            --team-id "$APPLE_TEAM_ID" \
            --password "$APPLE_APP_PASSWORD" \
            --wait
        xcrun stapler staple "$DMG"
    else
        log "Apple notarization credentials not set; skipping notarization"
    fi
fi

if [[ "$BUILD_PKG" -eq 1 ]]; then
    log "creating PKG"
    COMPONENT="$BUILD/component.pkg"
    RESOURCES="$BUILD/pkg-resources"
    PKG_ROOT="$BUILD/pkg-root"
    mkdir -p "$RESOURCES" "$PKG_ROOT/Applications"
    cp "$REPO_ROOT/LICENSE" "$RESOURCES/LICENSE"
    ditto --norsrc --noextattr "$APP" "$PKG_ROOT/Applications/$APP_NAME.app"
    codesign --verify --deep --strict "$PKG_ROOT/Applications/$APP_NAME.app"
    pkgbuild \
        --root "$PKG_ROOT" \
        --install-location / \
        --identifier "$BUNDLE_ID" \
        --version "$VERSION" \
        --ownership recommended \
        "$COMPONENT"

    cat > "$BUILD/distribution.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>$APP_NAME</title>
    <license file="LICENSE"/>
    <options customize="never" require-scripts="false"/>
    <choices-outline>
        <line choice="default"/>
    </choices-outline>
    <choice id="default" title="$APP_NAME" visible="false">
        <pkg-ref id="$BUNDLE_ID"/>
    </choice>
    <pkg-ref id="$BUNDLE_ID">component.pkg</pkg-ref>
</installer-gui-script>
EOF

    productbuild \
        --distribution "$BUILD/distribution.xml" \
        --resources "$RESOURCES" \
        --package-path "$BUILD" \
        "$PKG"

    if [[ -n "${PKG_SIGN_IDENTITY:-}" ]]; then
        productsign --sign "$PKG_SIGN_IDENTITY" "$PKG" "$BUILD/signed.pkg"
        mv "$BUILD/signed.pkg" "$PKG"
    fi
fi

(
    cd "$DIST"
    : > SHA256SUMS
    for artifact in "$DMG" "$PKG"; do
        [[ -f "$artifact" ]] || continue
        printf '%s  %s\n' "$(sha256_file "$artifact")" "$(basename "$artifact")" >> SHA256SUMS
    done
)

log "artifacts:"
ls -lh "$DMG"
if [[ -f "$PKG" ]]; then
    ls -lh "$PKG"
fi
log "done"
