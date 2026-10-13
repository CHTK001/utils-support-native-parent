#!/bin/bash
#
# Build the chua_native_imageopt cdylib on Linux / macOS.
#
# Usage:
#   ./build.sh                      # host platform, rustup default toolchain
#   ./build.sh linux x86_64         # explicit os/arch
#   ./build.sh darwin aarch64
#
# Requirements: cargo and a C compiler. No CMake and no vendored tarball are needed:
# the libdeflate and libimagequant C sources ship inside the libdeflater / imagequant
# crates and are compiled by the `cc` crate during the cargo build.
set -euo pipefail

RUST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_ROOT="$(cd "$RUST_DIR/.." && pwd)/resources/native"
LIBRARY_NAME="${LIBRARY_NAME:-chua_native_imageopt}"

OS_TYPE="${1:-auto}"
ARCH="${2:-auto}"

detect_os() {
    case "$OSTYPE" in
        linux-gnu*) echo linux ;;
        darwin*) echo darwin ;;
        msys*|cygwin*|win32*) echo windows ;;
        *) echo unknown ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo x86_64 ;;
        arm64|aarch64) echo aarch64 ;;
        *) echo x86_64 ;;
    esac
}

[ "$OS_TYPE" = auto ] && OS_TYPE="$(detect_os)"
[ "$ARCH" = auto ] && ARCH="$(detect_arch)"

case "$OS_TYPE/$ARCH" in
    linux/x86_64)   TARGET=x86_64-unknown-linux-gnu;  PLATFORM=linux-x86_64;   EXT=so;    PREFIX=lib ;;
    linux/aarch64)  TARGET=aarch64-unknown-linux-gnu; PLATFORM=linux-aarch64;  EXT=so;    PREFIX=lib ;;
    darwin/aarch64) TARGET=aarch64-apple-darwin;      PLATFORM=darwin-aarch64; EXT=dylib; PREFIX=lib ;;
    darwin/x86_64)  TARGET=x86_64-apple-darwin;        PLATFORM=darwin-x86_64;  EXT=dylib; PREFIX=lib ;;
    *) echo "[error] unsupported platform: $OS_TYPE/$ARCH" >&2; exit 1 ;;
esac

# 显式给了目标才安装对应 std；什么都不给就构建宿主目标，避免无谓的 rustup 下载。
if [ -n "$TARGET" ]; then
    if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
        echo "[build] installing rustup target $TARGET"
        rustup target add "$TARGET"
    fi
    echo "[build] cargo build --release --target $TARGET"
    ( cd "$RUST_DIR" && cargo build --release --target "$TARGET" )
    RELEASE_DIR="$RUST_DIR/target/$TARGET/release"
else
    echo "[build] cargo build --release (host target)"
    ( cd "$RUST_DIR" && cargo build --release )
    RELEASE_DIR="$RUST_DIR/target/release"
    case "$(uname -s)" in
        Darwin) PLATFORM="darwin-$(detect_arch)"; EXT=dylib; PREFIX=lib ;;
        MINGW*|MSYS*|CYGWIN*) PLATFORM="windows-x86_64"; EXT=dll; PREFIX="" ;;
        *) PLATFORM="linux-$(detect_arch)"; EXT=so; PREFIX=lib ;;
    esac
fi

BUILT="$RELEASE_DIR/${PREFIX}${LIBRARY_NAME}.${EXT}"
[ -f "$BUILT" ] || { echo "[error] artifact missing: $BUILT" >&2; exit 1; }
mkdir -p "$NATIVE_ROOT/$PLATFORM"
cp "$BUILT" "$NATIVE_ROOT/$PLATFORM/"
echo "[build] artifact: $NATIVE_ROOT/$PLATFORM/$(basename "$BUILT")"
