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
    elif [[ "$OSTYPE" == "darwin"* ]]; then echo "darwin"
    else echo "linux"; fi
}
detect_arch() {
    # 之前没有 aarch64|arm64 分支，在 Apple Silicon 上 auto 探测会静默退化成 x86_64。
    local arch=$(uname -m)
    case "$arch" in x86_64|amd64) echo "x86_64" ;; aarch64|arm64) echo "aarch64" ;; *) echo "x86_64" ;; esac
}

[[ "$OS_TYPE" == "auto" ]] && OS_TYPE=$(detect_os)
[[ "$ARCH" == "auto" ]] && ARCH=$(detect_arch)

# 补 darwin 之前这里没有 macOS 分支，native-matrix 的两个 darwin 腿都停在
# "Unsupported"，于是 darwin 产物无法在 CI 里重建。
# 平台矩阵必须覆盖 native-matrix.yml 声明的四个平台。
TARGET=""
case "$OS_TYPE-$ARCH" in
    windows-x86_64) TARGET="x86_64-pc-windows-msvc";   EXT="dll"   ; PLATFORM_DIR="windows-x86_64" ;;
    linux-x86_64)   TARGET="x86_64-unknown-linux-gnu"; EXT="so"    ; PLATFORM_DIR="linux-x86_64" ;;
    darwin-x86_64)  TARGET="x86_64-apple-darwin";      EXT="dylib" ; PLATFORM_DIR="darwin-x86_64" ;;
    darwin-aarch64) TARGET="aarch64-apple-darwin";     EXT="dylib" ; PLATFORM_DIR="darwin-aarch64" ;;
    *) echo "Unsupported: $OS_TYPE $ARCH"; exit 1 ;;
esac

rustup target add "$TARGET" 2>/dev/null || true
cargo build --release --target "$TARGET"

# 搬运产物。native-matrix 的后续步骤（Check exports / Stage artifact）读的都是
# <module>/src/main/resources/native/<platform>/ 下的文件，不搬运的话它们校验和上传的是
# 仓库里已入库的旧产物，而不是本次构建结果，校验等于失效。
# 本模块 Rust 源码在 src/rust 下，产物目录在 src/main/resources/native，所以回退到 ../main。
LIB_FILE="target/$TARGET/release/libchua_native_video_codec.$EXT"
if [ "$OS_TYPE" = "windows" ]; then LIB_FILE="target/$TARGET/release/chua_native_video_codec.$EXT"; fi
if [ ! -f "$LIB_FILE" ]; then
  echo "ERROR: build finished but artifact not found: $LIB_FILE" >&2
  exit 1
fi
DEST_DIR="$SCRIPT_DIR/../main/resources/native/$PLATFORM_DIR"
if [ ! -d "$(dirname "$DEST_DIR")" ]; then
  echo "ERROR: destination root does not exist: $(dirname "$DEST_DIR")" >&2
  exit 1
fi
mkdir -p "$DEST_DIR"
cp "$LIB_FILE" "$DEST_DIR/"
echo "Build: $DEST_DIR/$(basename "$LIB_FILE")"
