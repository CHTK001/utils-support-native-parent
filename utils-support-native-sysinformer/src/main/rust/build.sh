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

# 把实际使用的工具链版本打进日志。
#
# 为什么放在这里而不是 workflow 里单独一步：build.sh 是四平台**共用**的
# 构建入口，本地跑也走它。放在这里意味着**每一次**构建的日志都自带版本，
# 事后判断「产物为什么变了」时有据可查 —— 2026-10-03 那次漂移就是因为
# 日志里没有版本记录，只能靠事后加打印再去猜。
echo "rustc: $(rustc --version 2>&1)"
echo "cargo: $(cargo --version 2>&1)"

# Windows 的 PE 里带两处随链接时刻变化的字段：COFF TimeDateStamp 与
# CodeView(RSDS) 调试 GUID。缺了下面这个开关，同一份源码**每次编出的 dll
# 字节都不同**。
#
# 实测（2026-10-02）：852,480 字节里只有 24 字节不同 = 链接时间戳低位
# 2 + 另三个时间戳字段各 2 + PDB GUID 16，**.text 代码段 605,184 字节差异为 0**。
# 也就是说两次编出来功能等价，但 md5 不同，后果有二：
#   1. main 里入库的产物每跑一次 CI 就「落后」一次，且是**静默**的；
#   2. **无法用 md5 证明「交付的那份 == 验过的那份」**，而这正是验收报告
#      绑定产物的唯一硬凭据（Linux/macOS 三平台可以，只有 Windows 不行）。
# /Brepro 让链接器改用确定性算法生成校验和、时间戳与调试 GUID。
# 只对 MSVC 目标加，unix 链接器不认这个开关。
if [[ "$TARGET" == *"-pc-windows-msvc" ]]; then
    export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=/Brepro"
fi

# --locked 是必需的，缺了它「入库 Cargo.lock」这件事等于白做：
# 不加这个开关，cargo 仍然会按 `Cargo.toml` 的开放版本范围**静默更新**
# 锁文件（`serde = "1.0"`、`libc = "0.2"` 这类范围随时可能解析到新版本），
# 于是同一份源码在不同时间构建出的产物不同，而且没有任何提示。
# 加了它，一旦锁文件与 Cargo.toml 不一致就直接失败 —— 那是应该被看见的。
cargo build --locked --release --target "$TARGET"

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
