#!/bin/bash
# ============================================================
#  libsqlite3_hook.so / .dylib 构建脚本
#
#  不再需要 SQLite 合并包（运行时动态加载系统 sqlite3 库）
#
#  依赖：
#    1. GCC 或 Clang
#    2. SQLite 运行时库（Linux: libsqlite3.so, macOS: /usr/lib/libsqlite3.dylib）
#
#  用法：
#    chmod +x build.sh && ./build.sh
# ============================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

case "$OS" in
    linux)
        OUT_DIR="$SCRIPT_DIR/../resources/native/linux-$ARCH"
        LIB_NAME="libsqlite3_hook.so"
        LIBS="-lpthread -ldl"
        ;;
    darwin)
        OUT_DIR="$SCRIPT_DIR/../resources/native/osx-$ARCH"
        LIB_NAME="libsqlite3_hook.dylib"
        LIBS=""
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

mkdir -p "$OUT_DIR"

echo "[sqlite3_hook] Building $LIB_NAME..."

gcc -O2 -shared -fPIC \
    -I"$SCRIPT_DIR" \
    "$SCRIPT_DIR/sqlite3_hook.c" \
    -o "$OUT_DIR/$LIB_NAME" \
    $LIBS

echo "[sqlite3_hook] Build success: $OUT_DIR/$LIB_NAME"
ls -lh "$OUT_DIR/$LIB_NAME"
