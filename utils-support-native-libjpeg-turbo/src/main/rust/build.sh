#!/bin/bash
#
# Build the libjpeg-turbo static core and the chua_native_turbojpeg cdylib on Linux / macOS.
#
# Usage:
#   ./build.sh                      # host platform, release
#   ./build.sh linux x86_64         # explicit os/arch
#   ./build.sh darwin aarch64
#
# Requirements: cmake, nasm, cargo (+ rustup target for cross builds).
# On macOS the NASM-based SIMD kernels are skipped by upstream (ARM/Apple builds
# use the C fallback), so pass -- -DWITH_SIMD=OFF style flags via EXTRA_CMAKE_ARGS if needed.
set -euo pipefail

VERSION="${LJTB_VERSION:-3.1.2}"
RUST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR="$RUST_DIR/vendor"
SRC="$VENDOR/libjpeg-turbo-$VERSION"
LIB_DIR="$SRC/build"
NATIVE_ROOT="$(cd "$RUST_DIR/.." && pwd)/resources/native"

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
    linux/x86_64)  TARGET=x86_64-unknown-linux-gnu;   PLATFORM=linux-x86_64;  EXT=so;    PREFIX=lib ;;
    linux/aarch64) TARGET=aarch64-unknown-linux-gnu;  PLATFORM=linux-aarch64; EXT=so;    PREFIX=lib ;;
    darwin/aarch64) TARGET=aarch64-apple-darwin;      PLATFORM=darwin-aarch64; EXT=dylib; PREFIX=lib ;;
    darwin/x86_64) TARGET=x86_64-apple-darwin;        PLATFORM=darwin-x86_64; EXT=dylib; PREFIX=lib ;;
    *) echo "[error] unsupported platform: $OS_TYPE/$ARCH" >&2; exit 1 ;;
esac

EXTRA_CMAKE_ARGS=()
# darwin 上交叉出另一种架构：host 与 target 不一致时，上游的 SIMD 探测按 host 架构走，
# 会编进错指令集，故这种构建只出标量核。
if [ "$OS_TYPE" = darwin ]; then
    HOST_ARCH="$(detect_arch)"
    if [ "$HOST_ARCH" != "$ARCH" ]; then
        echo "[build] darwin cross: host=$HOST_ARCH target=$ARCH -> -DCMAKE_OSX_ARCHITECTURES=$ARCH, SIMD off"
        EXTRA_CMAKE_ARGS+=("-DCMAKE_OSX_ARCHITECTURES=$ARCH")
        WITH_SIMD=OFF
    fi
fi

mkdir -p "$VENDOR"
if [ ! -f "$LIB_DIR/libturbojpeg.a" ]; then
    if [ ! -f "$VENDOR/libjpeg-turbo-$VERSION.tar.gz" ]; then
        echo "[build] downloading libjpeg-turbo $VERSION source"
        curl -sSL -o "$VENDOR/libjpeg-turbo-$VERSION.tar.gz" \
            "https://github.com/libjpeg-turbo/libjpeg-turbo/archive/refs/tags/$VERSION.tar.gz"
    fi
    if [ ! -d "$SRC" ]; then
        echo "[build] extracting source"
        tar xzf "$VENDOR/libjpeg-turbo-$VERSION.tar.gz" -C "$VENDOR"
    fi
    echo "[build] configuring + building libjpeg-turbo (static, PIC, SIMD=${WITH_SIMD:-ON})"
    cmake -S "$SRC" -B "$LIB_DIR" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
        -DENABLE_SHARED=OFF -DENABLE_STATIC=ON \
        -DWITH_JPEG8=ON -DWITH_SIMD="${WITH_SIMD:-ON}" -DWITH_TURBOJPEG=ON \
        -DWITH_TOOLS=OFF -DWITH_TESTS=OFF -DWITH_FUZZ=OFF \
        ${EXTRA_CMAKE_ARGS[@]+"${EXTRA_CMAKE_ARGS[@]}"}
    cmake --build "$LIB_DIR"
fi

if ! rustup target list --installed | grep -q "^$TARGET\$"; then
    echo "[build] installing rustup target $TARGET"
    rustup target add "$TARGET"
fi

echo "[build] cargo build --release --target $TARGET"
(
    cd "$RUST_DIR"
    TURBOJPEG_LIB_DIR="$LIB_DIR" cargo build --release --target "$TARGET"
)

BUILT="$RUST_DIR/target/$TARGET/release/${PREFIX}chua_native_turbojpeg.$EXT"
[ -f "$BUILT" ] || { echo "[error] artifact missing: $BUILT" >&2; exit 1; }
mkdir -p "$NATIVE_ROOT/$PLATFORM"
cp "$BUILT" "$NATIVE_ROOT/$PLATFORM/"
echo "[build] artifact: $NATIVE_ROOT/$PLATFORM/$(basename "$BUILT")"
