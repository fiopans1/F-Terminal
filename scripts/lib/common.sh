#!/usr/bin/env bash
# Shared helpers for the native packaging scripts.
# This file is meant to be sourced, not executed.

# shellcheck disable=SC2034 # consumed by the scripts that source this file
if [[ -z "${REPO_ROOT:-}" ]]; then
    REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi

APP_NAME="F-Terminal"
BINARY_NAME="f-terminal"
PACKAGE_NAME="f-terminal"
BUNDLE_ID="com.fiopans1.f-terminal"
MAINTAINER="fiopans1 <fiopans1@users.noreply.github.com>"
HOMEPAGE="https://github.com/fiopans1/F-Terminal"
LICENSE_NAME="AGPL-3.0-only"

log() {
    printf '[package] %s\n' "$*" >&2
}

die() {
    printf '[package] error: %s\n' "$*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

app_version() {
    local version
    version="$(awk -F'"' '/^version[[:space:]]*=[[:space:]]*"/ { print $2; exit }' "$REPO_ROOT/Cargo.toml")"
    [[ -n "$version" ]] || die "could not read the version from Cargo.toml"
    printf '%s\n' "$version"
}

validate_version() {
    local version="$1"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
        || die "unsupported version format: $version (use X.Y.Z)"
}

sha256_file() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$file" | awk '{print $NF}'
    else
        die "no SHA-256 tool found (sha256sum, shasum or openssl)"
    fi
}

utc_date() {
    date -u +%Y-%m-%d
}

prepare_output_dir() {
    local dir="$1"
    mkdir -p "$dir"
}
