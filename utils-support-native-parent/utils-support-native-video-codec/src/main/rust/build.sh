#!/bin/bash
# Rust 原生视频编解码库构建脚本
# 用法: ./build.sh [platform] [arch] [mode]
# 示例: ./build.sh windows x86_64 release
#       ./build.sh linux x86_64 release
# 依赖: Rust 1.70+, 目标平台 C 编译器

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PLATFORM="${1:-auto}"
ARCH="${2:-auto}"
MODE="${3:-release}"

if [ "$PLATFORM" = "auto" ]; then
    case "$(uname -s)" in
        Linux*)  PLATFORM="linux";;
        Darwin*) PLATFORM="darwin";;
        CYGWIN*|MINGW*|MSYS*) PLATFORM="windows";;
        *) echo "Unknown platform: $(uname -s)"; exit 1;;
    esac
fi

if [ "$ARCH" = "auto" ]; then
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64) ARCH="x86_64";;
        aarch64|arm64) ARCH="aarch64";;
        *) echo "Unknown arch: $(uname -m)"; exit 1;;
    esac
fi

TARGET_DIR="target/${MODE}"
OUT_DIR="../resources/native/${PLATFORM}-${ARCH}"
mkdir -p "$OUT_DIR"

case "$PLATFORM" in
    windows)
        rustup target add x86_64-pc-windows-msvc
        cargo build --target x86_64-pc-windows-msvc ${MODE/release/--release}
        cp "${TARGET_DIR}/native_video_codec.dll" "$OUT_DIR/"
        ;;
    linux)
        rustup target add x86_64-unknown-linux-gnu
        cargo build --target x86_64-unknown-linux-gnu ${MODE/release/--release}
        cp "${TARGET_DIR}/libnative_video_codec.so" "$OUT_DIR/"
        ;;
    darwin)
        rustup target add aarch64-apple-darwin
        cargo build --target aarch64-apple-darwin ${MODE/release/--release}
        cp "${TARGET_DIR}/libnative_video_codec.dylib" "$OUT_DIR/"
        ;;
esac

echo "✅ 构建完成: $PLATFORM-$ARCH ($MODE)"
echo "   输出目录: $OUT_DIR"