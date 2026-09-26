#!/bin/bash

# wechat_wcdb Rust 原生库构建脚本
# 用法:
#   ./build.sh [平台] [架构] [构建模式]
# 示例:
#   ./build.sh linux x86_64 release      # Linux x86_64 发布版（产物 libwechat_wcdb.so）
#   ./build.sh windows x86_64 release    # Windows x86_64 发布版（产物 wechat_wcdb.dll）
#   ./build.sh darwin aarch64 release    # macOS ARM64 发布版
#   ./build.sh auto auto release         # 自动检测平台和架构
#
# 依赖（vendored SQLCipher 会从源码构建 OpenSSL）：
#   Linux : build-essential perl pkg-config
#   macOS : perl cc（runner 自带）
#   Windows: nasm + 可用的 perl
#   产物落在 src/main/resources/native/{platform}/，
#   由 .github/workflows/native-wechat.yml 负责多平台构建与回填。

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

OS_TYPE="${1:-auto}"
ARCH="${2:-auto}"
BUILD_MODE="${3:-release}"

detect_os() {
    # 用 case 而非 [[ ]]：原先的 [[ "$OSTYPE" == "linux-gnu""* ]] 中相邻引号拼接
    # 会让 bash 的条件表达式解析器失衡，表现为 line 28 语法错误并吞掉后续多行，
    # 在 CI 上四个平台全部以 exit code 2 秒挂。case 无此歧义。
    case "$OSTYPE" in
        msys*|cygwin*|win32*) echo "windows" ;;
        linux-gnu*)           echo "linux" ;;
        darwin*)              echo "darwin" ;;
        *)                    echo "unknown" ;;
    esac
}

detect_arch() {
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) echo "x86_64" ;;
    esac
}

if [[ "$OS_TYPE" == "auto" ]]; then
    OS_TYPE=$(detect_os)
    echo -e "${YELLOW}[INFO]${NC} 自动检测操作系统: $OS_TYPE"
fi
if [[ "$ARCH" == "auto" ]]; then
    ARCH=$(detect_arch)
    echo -e "${YELLOW}[INFO]${NC} 自动检测架构: $ARCH"
fi
if [[ "$BUILD_MODE" != "release" && "$BUILD_MODE" != "debug" ]]; then
    echo -e "${RED}[ERROR]${NC} 无效的构建模式: $BUILD_MODE"
    exit 1
fi

setup_target() {
    local target="" lib_ext="" platform_dir=""
    case "$OS_TYPE" in
        windows)
            case "$ARCH" in
                x86_64) target="x86_64-pc-windows-msvc"; lib_ext="dll"; platform_dir="windows-x86_64" ;;
                *) echo -e "${RED}[ERROR]${NC} Windows 不支持架构: $ARCH"; exit 1 ;;
            esac ;;
        linux)
            case "$ARCH" in
                x86_64) target="x86_64-unknown-linux-gnu"; lib_ext="so"; platform_dir="linux-x86_64" ;;
                aarch64) target="aarch64-unknown-linux-gnu"; lib_ext="so"; platform_dir="linux-aarch64" ;;
                *) echo -e "${RED}[ERROR]${NC} Linux 不支持架构: $ARCH"; exit 1 ;;
            esac ;;
        darwin)
            case "$ARCH" in
                x86_64) target="x86_64-apple-darwin"; lib_ext="dylib"; platform_dir="darwin-x86_64" ;;
                aarch64) target="aarch64-apple-darwin"; lib_ext="dylib"; platform_dir="darwin-aarch64" ;;
                *) echo -e "${RED}[ERROR]${NC} macOS 不支持架构: $ARCH"; exit 1 ;;
            esac ;;
        *) echo -e "${RED}[ERROR]${NC} 不支持的操作系统: $OS_TYPE"; exit 1 ;;
    esac
    export TARGET="$target" LIB_EXT="$lib_ext" PLATFORM_DIR="$platform_dir"
    echo -e "${GREEN}[INFO]${NC} 目标平台: $TARGET"
}

check_cargo_toml() {
    if [[ ! -f "Cargo.toml" ]]; then echo -e "${RED}[ERROR]${NC} 未找到 Cargo.toml"; exit 1; fi
    # 用 awk 而非 grep -E '^name\s*='：\s 是 GNU grep 扩展，macOS 自带 BSD grep
    # 不支持，会静默匹配不到，导致 LIB_NAME 为空、后续报"未找到动态库"这种
    # 误导性错误。awk 是 POSIX 工具，Linux / macOS / Git Bash 行为一致。
    # 只取 [lib] 段内的 name，避免与 [package] name 混淆。
    LIB_NAME=$(awk '
        /^\[lib\]/            { inlib = 1; next }
        /^\[/                 { inlib = 0 }
        inlib && /^[[:space:]]*name[[:space:]]*=/ {
            sub(/^[^=]*=[[:space:]]*"/, "")
            sub(/".*$/, "")
            print
            exit
        }
    ' Cargo.toml)
    if [ -z "$LIB_NAME" ]; then
        echo -e "${RED}[ERROR]${NC} 未能从 Cargo.toml 的 [lib] 段解析出库名"
        exit 1
    fi
    echo -e "${GREEN}[INFO]${NC} 库名称: $LIB_NAME"
}

build_project() {
    echo -e "\n${GREEN}========================================${NC}"
    echo -e "${GREEN}[BUILD]${NC} 开始编译: wechat_wcdb"
    local build_cmd="cargo build"
    [[ "$BUILD_MODE" == "release" ]] && build_cmd="$build_cmd --release"
    build_cmd="$build_cmd --target $TARGET"
    echo -e "${GREEN}[INFO]${NC} 执行: $build_cmd"
    eval "$build_cmd" || { echo -e "${RED}[ERROR]${NC} 编译失败"; exit 1; }
}

find_and_copy_lib() {
    local target_dir="target/$TARGET/$BUILD_MODE" lib_file=""
    case "$OS_TYPE" in
        windows) lib_file="$target_dir/${LIB_NAME}.dll" ;;
        linux) lib_file="$target_dir/lib${LIB_NAME}.so" ;;
        darwin) lib_file="$target_dir/lib${LIB_NAME}.dylib" ;;
    esac
    [[ -f "$lib_file" ]] || { echo -e "${RED}[ERROR]${NC} 未找到动态库: $lib_file"; exit 1; }
    local native_dir="$SCRIPT_DIR/../resources/native/$PLATFORM_DIR"
    mkdir -p "$native_dir"
    cp "$lib_file" "$native_dir/"
    echo -e "${GREEN}[SUCCESS]${NC} 复制到: $native_dir/"
}

main() {
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}wechat_wcdb Rust 原生库构建脚本${NC}"
    echo -e "${GREEN}========================================${NC}"
    check_cargo_toml
    setup_target
    if ! command -v cargo &>/dev/null; then echo -e "${RED}[ERROR]${NC} 未找到 cargo"; exit 1; fi
    if ! rustup target list --installed | grep -q "^$TARGET$"; then
        echo -e "${YELLOW}[INFO]${NC} 安装目标平台: $TARGET"
        rustup target add "$TARGET"
    fi
    build_project
    find_and_copy_lib
    echo -e "\n${GREEN}[SUCCESS]${NC} 构建完成！"
}

main "$@"
