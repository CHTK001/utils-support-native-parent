#!/bin/bash

# uia_rust Rust 原生库构建脚本
# 用法:
#   ./build.sh [平台] [架构] [构建模式]
# 示例:
#   ./build.sh windows x86_64 release      # Windows x86_64 发布版（产物 uia_rust.dll）
#   ./build.sh auto auto release         # 自动检测平台和架构
#   ./build.sh auto auto debug            # 调试版

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

# 离线构建开关：默认 0（联网拉取 crates）。
# 只有在确认本地 registry 已有全部依赖时才置 1；CI 上必须为 0，
# 否则干净 runner 会因缺少缓存而直接失败。
CARGO_OFFLINE_FLAG="${CARGO_OFFLINE:-0}"

detect_os() {
    if   [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* || "$OSTYPE" == win32* ]]; then echo windows
    elif [[ "$OSTYPE" == linux-gnu* ]]; then echo linux
    elif [[ "$OSTYPE" == darwin* ]]; then echo darwin
    else echo unknown; fi
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo x86_64 ;;
        aarch64|arm64) echo aarch64 ;;
        *) echo x86_64 ;;
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

# UIA 是 Windows 独有技术，源码内 #[cfg(windows)] 门控，
# 因此其他平台不参与产物分发
setup_target() {
    local target="" lib_ext="" platform_dir=""
    case "$OS_TYPE" in
        windows)
            case "$ARCH" in
                x86_64)  target="x86_64-pc-windows-msvc"; lib_ext="dll"; platform_dir="windows-x86_64" ;;
                aarch64) target="aarch64-pc-windows-msvc"; lib_ext="dll"; platform_dir="windows-aarch64" ;;
                *) echo -e "${RED}[ERROR]${NC} Windows 不支持架构: $ARCH"; exit 1 ;;
            esac ;;
        *) echo -e "${RED}[ERROR]${NC} UIA 库仅支持 Windows，当前: $OS_TYPE"; exit 1 ;;
    esac
    export TARGET="$target" LIB_EXT="$lib_ext" PLATFORM_DIR="$platform_dir"
    echo -e "${GREEN}[INFO]${NC} 目标平台: $TARGET"
}

check_cargo_toml() {
    if [[ ! -f "Cargo.toml" ]]; then echo -e "${RED}[ERROR]${NC} 未找到 Cargo.toml"; exit 1; fi
    LIB_NAME=$(grep -A1 '^\[lib\]' Cargo.toml | grep -E '^name\s*=' | head -1 \
        | sed -E 's/^name\s*=\s*"([^"]+)".*/\1/')
    echo -e "${GREEN}[INFO]${NC} 库名: $LIB_NAME"
}

build_project() {
    echo -e "\n${GREEN}========================================${NC}"
    echo -e "${GREEN}[BUILD]${NC} 开始编译 $LIB_NAME"
    local build_cmd="cargo build"
    [[ "$BUILD_MODE" == "release" ]] && build_cmd="$build_cmd --release"
    build_cmd="$build_cmd --target $TARGET"
    [[ "$CARGO_OFFLINE_FLAG" == "1" ]] && build_cmd="$build_cmd --offline"
    echo -e "${GREEN}[INFO]${NC} 执行: $build_cmd"
    eval "$build_cmd" || { echo -e "${RED}[ERROR]${NC} 编译失败"; exit 1; }
}

find_and_copy_lib() {
    local target_dir="target/$TARGET/$BUILD_MODE" lib_file=""
    lib_file="$target_dir/${LIB_NAME}.${LIB_EXT}"
    [[ -f "$lib_file" ]] || { echo -e "${RED}[ERROR]${NC} 未找到动态库: $lib_file"; exit 1; }
    local native_dir="$SCRIPT_DIR/../resources/native/$PLATFORM_DIR"
    mkdir -p "$native_dir"
    cp "$lib_file" "$native_dir/"
    echo -e "${GREEN}[SUCCESS]${NC} 已复制到 $native_dir/"
}

main() {
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}uia_rust Rust 原生库构建脚本${NC}"
    echo -e "${GREEN}========================================${NC}"
    check_cargo_toml
    setup_target
    if ! command -v cargo &>/dev/null; then echo -e "${RED}[ERROR]${NC} 未找到 cargo"; exit 1; fi
    if ! rustup target list --installed 2>/dev/null | grep -q "^$TARGET$"; then
        echo -e "${YELLOW}[INFO]${NC} 安装目标平台: $TARGET"
        rustup target add "$TARGET"
    fi
    build_project
    find_and_copy_lib
    echo -e "\n${GREEN}[SUCCESS]${NC} 构建完成"
}

main "$@"
