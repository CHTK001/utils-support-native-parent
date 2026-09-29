#!/bin/bash
# ============================================================
#  sysinformer 四平台构建脚本
#
#  用法：
#    ./build.sh                          按当前主机构建
#    ./build.sh linux   x86_64 release   指定目标（供 CI 调用）
#    ./build.sh darwin  aarch64
#
#  源在 src/main/rust，产物落在 src/main/resources/native/<平台>/
# ============================================================
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
    # uname 在 Apple Silicon 上返回 arm64，而消费方按 aarch64 分目录，
    # 必须归一化，否则产物会被写进没人读取的 native/darwin-arm64/。
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) echo "x86_64" ;;
    esac
}

[[ "$OS_TYPE" == "auto" ]] && OS_TYPE=$(detect_os)
[[ "$ARCH" == "auto" ]] && ARCH=$(detect_arch)

TARGET=""
EXT=""
PLATFORM_DIR=""
case "$OS_TYPE-$ARCH" in
    windows-x86_64)   TARGET="x86_64-pc-windows-msvc";    EXT="dll"   ; PLATFORM_DIR="windows-x86_64" ;;
    linux-x86_64)     TARGET="x86_64-unknown-linux-gnu";  EXT="so"    ; PLATFORM_DIR="linux-x86_64" ;;
    linux-aarch64)    TARGET="aarch64-unknown-linux-gnu"; EXT="so"    ; PLATFORM_DIR="linux-aarch64" ;;
    darwin-x86_64)    TARGET="x86_64-apple-darwin";       EXT="dylib" ; PLATFORM_DIR="darwin-x86_64" ;;
    darwin-aarch64)   TARGET="aarch64-apple-darwin";      EXT="dylib" ; PLATFORM_DIR="darwin-aarch64" ;;
    *) echo "Unsupported: $OS_TYPE $ARCH" >&2; exit 1 ;;
esac

rustup target add "$TARGET" 2>/dev/null || true
cargo build --release --target "$TARGET"

# 产物名：windows 无 lib 前缀，unix 有
if [ "$OS_TYPE" = "windows" ]; then
    LIB_FILE="target/$TARGET/release/sysinformer.$EXT"
else
    LIB_FILE="target/$TARGET/release/libsysinformer.$EXT"
fi

# 构建返回 0 但产物不存在的情况真实发生过，显式校验而不是让后续步骤读到旧产物
if [ ! -f "$LIB_FILE" ]; then
    echo "ERROR: 构建完成但产物不存在: $LIB_FILE" >&2
    exit 1
fi

# 源在 src/main/rust，产物目录在 src/main/resources/native。
#
# 注意相对层级：从 src/main/rust 上跳**一级**才到 src/main，所以是 ../resources。
# 曾写成 ../../resources，解析后落在 src/resources/native —— 一个没人读的目录。
# 当时没发现，是因为守卫只比对了字符串形状（../.. 原样出现在期望串里，自然相等），
# 比中了错的位置。现在改为**先解析成绝对路径再校验后缀**，写错层级必然被拦。
MODULE_MAIN="$(cd "$SCRIPT_DIR/.." && pwd)"
DEST_ROOT="$MODULE_MAIN/resources/native"
case "$DEST_ROOT" in
    */src/main/resources/native) : ;;
    *)
        echo "ERROR: 产物路径异常，期望以 src/main/resources/native 结尾，实际为: $DEST_ROOT" >&2
        echo "       （多半是 build.sh 里的相对层级写错，请核对 src/main/rust 到 src/main 的跳数）" >&2
        exit 1
        ;;
esac
if ! mkdir -p "$DEST_ROOT"; then
    echo "ERROR: 无法创建产物根目录: $DEST_ROOT" >&2
    exit 1
fi
DEST_DIR="$DEST_ROOT/$PLATFORM_DIR"
mkdir -p "$DEST_DIR"
cp "$LIB_FILE" "$DEST_DIR/"
echo "Build complete: $DEST_DIR/$(basename "$LIB_FILE")"
