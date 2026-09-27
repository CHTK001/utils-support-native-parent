#!/bin/bash
#
# Rust 模块构建脚本
#
# 用法:
#   ./build.sh [平台] [架构] [构建模式]
# 示例:
#   ./build.sh windows x86_64 release
#   ./build.sh linux   x86_64 release
#   ./build.sh darwin  aarch64 release
#   ./build.sh auto    auto   release
#
# 构建完成后把动态库复制到 ../resources/native/<平台目录>/，
# 平台目录名与 Maven 资源布局一致（windows-x86_64 / linux-x86_64 /
# darwin-x86_64 / darwin-aarch64）。

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
    if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]; then
        echo "windows"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        echo "linux"
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        echo "darwin"
    else
        echo "unknown"
    fi
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
    echo -e "${YELLOW}[INFO]${NC} 自动检测系统: $OS_TYPE"
fi

if [[ "$ARCH" == "auto" ]]; then
    ARCH=$(detect_arch)
    echo -e "${YELLOW}[INFO]${NC} 自动检测架构: $ARCH"
fi

if [[ "$BUILD_MODE" != "release" && "$BUILD_MODE" != "debug" ]]; then
    echo -e "${RED}[ERROR]${NC} 无效的构建模式: $BUILD_MODE"
    exit 1
fi

case "$OS_TYPE" in
    windows)
        case "$ARCH" in
            x86_64) TARGET="x86_64-pc-windows-msvc"; PLATFORM_DIR="windows-x86_64" ;;
            *) echo -e "${RED}[ERROR]${NC} Windows 不支持架构: $ARCH"; exit 1 ;;
        esac
        ;;
    linux)
        case "$ARCH" in
            x86_64) TARGET="x86_64-unknown-linux-gnu"; PLATFORM_DIR="linux-x86_64" ;;
            aarch64) TARGET="aarch64-unknown-linux-gnu"; PLATFORM_DIR="linux-aarch64" ;;
            *) echo -e "${RED}[ERROR]${NC} Linux 不支持架构: $ARCH"; exit 1 ;;
        esac
        ;;
    darwin)
        case "$ARCH" in
            x86_64) TARGET="x86_64-apple-darwin"; PLATFORM_DIR="darwin-x86_64" ;;
            aarch64) TARGET="aarch64-apple-darwin"; PLATFORM_DIR="darwin-aarch64" ;;
            *) echo -e "${RED}[ERROR]${NC} macOS 不支持架构: $ARCH"; exit 1 ;;
        esac
        ;;
    *)
        echo -e "${RED}[ERROR]${NC} 不支持的操作系统: $OS_TYPE"
        exit 1
        ;;
esac

echo -e "${GREEN}[INFO]${NC} 目标平台: $TARGET -> native/$PLATFORM_DIR/"

if ! command -v cargo >/dev/null 2>&1; then
    echo -e "${RED}[ERROR]${NC} 未找到 cargo"
    exit 1
fi

if ! rustup target list --installed | grep -q "^$TARGET$"; then
    echo -e "${YELLOW}[INFO]${NC} 安装目标平台: $TARGET"
    rustup target add "$TARGET"
fi

if [[ "$BUILD_MODE" == "release" ]]; then
    BUILD_CMD="cargo build --release --target $TARGET"
else
    BUILD_CMD="cargo build --target $TARGET"
fi

echo -e "${GREEN}[INFO]${NC} 执行: $BUILD_CMD"
eval "$BUILD_CMD"

TARGET_DIR="target/$TARGET/$BUILD_MODE"

# 用扩展名定位产物而不是拼包名：包名与 [lib] name 不一致时（例如
# rust_filesearch 的 [lib] name 是 file_search）拼包名会找不到文件。
LIB_FILE=""
for ext in dll so dylib; do
    candidate=$(find "$TARGET_DIR" -maxdepth 1 -type f -name "*.$ext" | head -n 1)
    if [[ -n "$candidate" ]]; then
        LIB_FILE="$candidate"
        break
    fi
done

if [[ -z "$LIB_FILE" ]]; then
    echo -e "${RED}[ERROR]${NC} 未在 $TARGET_DIR 找到动态库"
    ls -la "$TARGET_DIR" || true
    exit 1
fi

NATIVE_DIR=""
if [[ -d "$SCRIPT_DIR/../main/resources" ]]; then
    # 项目位于 src/rust/（如 metrics / video-codec），资源在 src/main/resources/
    NATIVE_DIR="$SCRIPT_DIR/../main/resources/native/$PLATFORM_DIR"
else
    # 项目位于 src/main/rust/（多数模块），资源在 src/main/resources/
    NATIVE_DIR="$SCRIPT_DIR/../resources/native/$PLATFORM_DIR"
fi
mkdir -p "$NATIVE_DIR"
cp "$LIB_FILE" "$NATIVE_DIR/"
echo -e "${GREEN}[SUCCESS]${NC} $(basename "$LIB_FILE") -> ${NATIVE_DIR#$SCRIPT_DIR/}"
