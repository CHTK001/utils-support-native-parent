#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

OS_TYPE="${1:-auto}"
ARCH="${2:-auto}"
BUILD_MODE="${3:-release}"

detect_os() {
    if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]; then echo "windows"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then echo "linux"
    else echo "linux"; fi
}
detect_arch() {
    local arch=$(uname -m)
    case "$arch" in x86_64|amd64) echo "x86_64" ;; *) echo "x86_64" ;; esac
}

[[ "$OS_TYPE" == "auto" ]] && OS_TYPE=$(detect_os)
[[ "$ARCH" == "auto" ]] && ARCH=$(detect_arch)

TARGET=""
case "$OS_TYPE-$ARCH" in
    windows-x86_64) TARGET="x86_64-pc-windows-msvc"; EXT="dll" ;;
    linux-x86_64) TARGET="x86_64-unknown-linux-gnu"; EXT="so" ;;
    *) echo "Unsupported"; exit 1 ;;
esac

rustup target add "$TARGET" 2>/dev/null || true
cargo build --release --target "$TARGET"
echo "Build: target/$TARGET/release/libchua_native_video_codec.$EXT"
