#!/bin/sh
set -eu

fail() {
    echo "error: $*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
PLUGIN_ROOT=${HERDR_PLUGIN_ROOT:-$REPO_ROOT}
VERSION_FILE="$REPO_ROOT/VERSION"

[ -f "$VERSION_FILE" ] || fail "VERSION file not found at $VERSION_FILE"

need_cmd tr
need_cmd uname

VERSION=$(tr -d '[:space:]' < "$VERSION_FILE")
[ -n "$VERSION" ] || fail "VERSION file is empty"

case "$(uname -s):$(uname -m)" in
    Darwin:arm64|Darwin:aarch64)
        TARGET="aarch64-apple-darwin"
        ;;
    Darwin:x86_64)
        TARGET="x86_64-apple-darwin"
        ;;
    Linux:x86_64|Linux:amd64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
    *)
        fail "unsupported platform: $(uname -s) $(uname -m)"
        ;;
esac

need_cmd curl
need_cmd tar
need_cmd grep
need_cmd cut
need_cmd sed
need_cmd head
need_cmd mktemp
need_cmd mkdir
need_cmd cp
need_cmd chmod
need_cmd mv

if command -v sha256sum >/dev/null 2>&1; then
    SHA256_TOOL="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
    SHA256_TOOL="shasum"
else
    fail "required command not found: sha256sum or shasum"
fi

INSTALL_DIR="$PLUGIN_ROOT/bin"
BIN="$INSTALL_DIR/herdr-scratch"

if [ -x "$BIN" ]; then
    INSTALLED_VERSION=$("$BIN" --version 2>/dev/null | sed -n 's/^herdr-scratch //p' | head -n 1 || true)
    if [ "$INSTALLED_VERSION" = "$VERSION" ]; then
        echo "herdr-scratch $VERSION already installed at $BIN"
        exit 0
    fi
fi

RELEASE_REPO=${HERDR_SCRATCH_RELEASE_REPO:-AkashJana18/herdr-scratch}
RELEASE_BASE_URL=${HERDR_SCRATCH_RELEASE_BASE_URL:-https://github.com/$RELEASE_REPO/releases/download/v$VERSION}
ARCHIVE="herdr-scratch-$TARGET.tar.gz"
TMP_PARENT=${TMPDIR:-/tmp}
TMP_DIR=$(mktemp -d "$TMP_PARENT/herdr-scratch-install.XXXXXX")

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT HUP INT TERM

echo "installing herdr-scratch $VERSION for $TARGET"
curl -fsSL "$RELEASE_BASE_URL/checksums.txt" -o "$TMP_DIR/checksums.txt" \
    || fail "failed to download checksums from $RELEASE_BASE_URL/checksums.txt"
curl -fsSL "$RELEASE_BASE_URL/$ARCHIVE" -o "$TMP_DIR/$ARCHIVE" \
    || fail "failed to download release asset $ARCHIVE"

CHECKSUM_LINE=$(grep "  $ARCHIVE\$" "$TMP_DIR/checksums.txt" || true)
[ -n "$CHECKSUM_LINE" ] || fail "checksum for $ARCHIVE not found in checksums.txt"

EXPECTED_HASH=$(printf '%s\n' "$CHECKSUM_LINE" | cut -d ' ' -f 1)
if [ "$SHA256_TOOL" = "sha256sum" ]; then
    ACTUAL_HASH=$(sha256sum "$TMP_DIR/$ARCHIVE" | cut -d ' ' -f 1)
else
    ACTUAL_HASH=$(shasum -a 256 "$TMP_DIR/$ARCHIVE" | cut -d ' ' -f 1)
fi

[ "$EXPECTED_HASH" = "$ACTUAL_HASH" ] || fail "checksum verification failed for $ARCHIVE"

mkdir -p "$TMP_DIR/extract" "$INSTALL_DIR"
tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR/extract" \
    || fail "failed to extract $ARCHIVE"
[ -f "$TMP_DIR/extract/herdr-scratch" ] || fail "archive did not contain herdr-scratch"

cp "$TMP_DIR/extract/herdr-scratch" "$BIN.tmp"
chmod +x "$BIN.tmp"
mv "$BIN.tmp" "$BIN"

echo "installed herdr-scratch $VERSION to $BIN"
