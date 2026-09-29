#!/bin/bash
# ============================================================
#  libsqlite3_hook.so / .dylib 构建脚本
#
#  不再需要 SQLite 合并包（运行时动态加载系统 sqlite3 库）
#
#  用法：
#    ./build.sh                          按当前主机构建
#    ./build.sh linux    x86_64          指定目标
#    ./build.sh darwin   aarch64
#    ./build.sh darwin   x86_64          在 Apple Silicon 上交叉编译
#    ./build.sh <os> <arch> [debug|release]
#
#  依赖：
#    1. GCC 或 Clang
#    2. SQLite 运行时库（Linux: libsqlite3.so, macOS: /usr/lib/libsqlite3.dylib）
#    3. Linux: liburing（io_uring 异步 I/O 支持），CI 上需 liburing-dev
#
#  源文件分工（不要改错）：
#    sqlite3_hook.c       Windows 走 IOCP 分支，Linux 走 io_uring 分支
#                         （两个 hook_open_async 各自在 #ifdef _WIN32 / #else 内，
#                           是有意的两套实现，不是重复定义）
#    sqlite3_hook_macos.c macOS 没有 io_uring，改用 POSIX pipe + select
#    sqlite3_hook_linux.c 零引用的历史残留，只有 4 个导出且缺 hook_wait 与
#                         三个 async 导出，任何构建入口都不应引用它
# ============================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
NATIVE_DIR="$SCRIPT_DIR/../resources/native"

# ---------- 目标平台与架构 ----------
TARGET_OS="${1:-}"
TARGET_ARCH="${2:-}"
PROFILE="${3:-release}"

if [ -z "$TARGET_OS" ]; then
    TARGET_OS=$(uname -s | tr '[:upper:]' '[:lower:]')
fi
if [ -z "$TARGET_ARCH" ]; then
    TARGET_ARCH=$(uname -m)
fi

# uname 在 Apple Silicon 上返回 arm64，而消费方（Java NativeLoader 与
# native-image resource-config.json）按 aarch64 分目录。早期脚本直接拿
# uname -m 拼路径，于是 macOS arm64 runner 把产物写进 native/darwin-arm64/
# ——一个没有任何代码会去读的目录，CI 看上去构建成功，运行时却找不到库。
# 这里统一归一化成 aarch64，并同时接受两种写法。
case "$TARGET_ARCH" in
    arm64|aarch64) TARGET_ARCH="aarch64" ;;
    x86_64|amd64)  TARGET_ARCH="x86_64" ;;
    *)
        echo "错误: 不支持的架构 '$TARGET_ARCH'（只支持 x86_64 / aarch64）"
        exit 1
        ;;
esac

case "$TARGET_OS" in
    linux)
        OUT_DIR="$NATIVE_DIR/linux-$TARGET_ARCH"
        LIB_NAME="libsqlite3_hook.so"
        SRC_FILE="sqlite3_hook.c"
        LIBS="-lpthread -ldl -luring"
        ;;
    darwin)
        OUT_DIR="$NATIVE_DIR/darwin-$TARGET_ARCH"
        LIB_NAME="libsqlite3_hook.dylib"
        SRC_FILE="sqlite3_hook_macos.c"
        LIBS="-lpthread"
        ;;
    *)
        echo "错误: 不支持的操作系统 '$TARGET_OS'（只支持 linux / darwin；Windows 请用 build.bat）"
        exit 1
        ;;
esac

# ---------- 交叉编译 ----------
# macOS 的 clang 用 -arch 直接切目标架构，这是 Apple Silicon 上产出 Intel
# dylib 的正规做法（macos-15 runner 只有 arm64，darwin-x86_64 这条腿全靠它）。
# Linux 侧不做跨架构交叉：仓库里没有 x86_64 交叉工具链，linux-x86_64 由
# ubuntu runner 原生构建。
ARCH_FLAGS=""
if [ "$TARGET_OS" = "darwin" ]; then
    if [ "$(uname -s)" != "Darwin" ]; then
        echo "错误: 非 macOS 主机无法构建 darwin 目标（实际 $(uname -s)）"
        exit 1
    fi
    case "$TARGET_ARCH" in
        x86_64) ARCH_FLAGS="-arch x86_64" ;;
        aarch64) ARCH_FLAGS="-arch arm64" ;;
    esac
    CC_BIN="${CC:-clang}"
else
    CC_BIN="${CC:-gcc}"
fi

if ! command -v "$CC_BIN" >/dev/null 2>&1; then
    echo "错误: 找不到编译器 '$CC_BIN'"
    exit 1
fi

# ---------- 优化级别 ----------
case "$PROFILE" in
    debug)   OPT_FLAGS="-O0 -g" ;;
    release) OPT_FLAGS="-O2" ;;
    *) echo "错误: 未知的 profile '$PROFILE'（只支持 debug / release）"; exit 1 ;;
esac

# ---------- 目录守卫 ----------
# 目录建不出来却继续往下走，最后的 ls -lh 才会报错，报错信息里看不出
# 真正的原因是路径不存在。这里提前失败并把路径打出来。
if ! mkdir -p "$OUT_DIR"; then
    echo "错误: 无法创建产物目录 '$OUT_DIR'"
    echo "  NATIVE_DIR 解析为 '$NATIVE_DIR'"
    echo "  确认 src/main/c/ 相对 src/main/resources/native/ 的层级没被改动"
    exit 1
fi

OUT_PATH="$OUT_DIR/$LIB_NAME"

echo "[sqlite3_hook] 目标    = $TARGET_OS/$TARGET_ARCH ($PROFILE)"
echo "[sqlite3_hook] 源文件  = $SRC_FILE"
echo "[sqlite3_hook] 编译器  = $CC_BIN $ARCH_FLAGS $OPT_FLAGS"
echo "[sqlite3_hook] 输出    = $OUT_PATH"

"$CC_BIN" $OPT_FLAGS -shared -fPIC $ARCH_FLAGS \
    -I"$SCRIPT_DIR" \
    "$SCRIPT_DIR/$SRC_FILE" \
    -o "$OUT_PATH" \
    $LIBS

# 构建命令返回 0 但产物不存在的情况是真实发生过的（交叉编译参数拼错时编译器
# 仍可能成功退出）。显式校验，避免"构建成功"与"产物在"之间出现盲区。
if [ ! -f "$OUT_PATH" ]; then
    echo "错误: 编译命令返回成功，但产物不存在: $OUT_PATH"
    exit 1
fi

echo "[sqlite3_hook] Build success: $OUT_PATH"
ls -lh "$OUT_PATH"
