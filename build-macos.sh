#!/usr/bin/env bash
# ============================================================
# utils-support-native-parent → macOS (osxcross) 交叉编译脚本
#
# 依赖：
#   - Linux x86_64 主机 + osxcross（含 macOS SDK）
#     OSXCROSS_ROOT 默认 /opt/osxcross，可用环境变量覆盖
#   - rustup / cargo（rustup target add aarch64-apple-darwin）
#   - clang（osxcross 自带，通过 o64-clang / oa64-clang 或
#     x86_64-apple-darwin-clang / aarch64-apple-darwin-clang 提供）
#
# 用法：
#   chmod +x build-macos.sh
#   ./build-macos.sh            # 默认 aarch64 (Apple Silicon)
#   ./build-macos.sh aarch64    # Apple Silicon
#   ./build-macos.sh x86_64     # Intel Mac
#   ./build-macos.sh aarch64 x86_64   # 双架构
# ============================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_SRC_BASE="$SCRIPT_DIR"
JNI_HEADERS="$SCRIPT_DIR/jni-headers/darwin"
CARGO_TARGET_DIR="$SCRIPT_DIR/target-macos"
OUTPUT_BASE="$SCRIPT_DIR"

# ---- osxcross 定位 ----
# 标准 osxcross ./build.sh 将工具链放到 <root>/target/bin；
# 若手动整理为 <root>/bin（CI 常用），同样支持。
OSXCROSS_ROOT="${OSXCROSS_ROOT:-/opt/osxcross}"
if [ -d "$OSXCROSS_ROOT/target/bin" ]; then
    OSXCROSS_BIN="$OSXCROSS_ROOT/target/bin"
elif [ -d "$OSXCROSS_ROOT/bin" ]; then
    OSXCROSS_BIN="$OSXCROSS_ROOT/bin"
else
    echo "[ERROR] osxcross toolchain not found under $OSXCROSS_ROOT"
    echo "        run ./build.sh first, or export OSXCROSS_ROOT=/path/to/osxcross"
    exit 1
fi
export PATH="$OSXCROSS_BIN:$PATH"
echo "[INFO] osxcross bin: $OSXCROSS_BIN"

# osxcross 自带的 libxar/libtapi 运行时库（osxcross ld 依赖 libxar.so.1）
OSXCROSS_LIB="$(dirname "$OSXCROSS_BIN")/lib"
[ -d "$OSXCROSS_LIB" ] || OSXCROSS_LIB="$OSXCROSS_ROOT/lib"
[ -d "$OSXCROSS_LIB" ] || OSXCROSS_LIB="$OSXCROSS_ROOT/target/lib"
if [ -d "$OSXCROSS_LIB" ]; then
    export LD_LIBRARY_PATH="$OSXCROSS_LIB:$LD_LIBRARY_PATH"
    echo "[INFO] osxcross lib: $OSXCROSS_LIB"
fi

# ---- 目标架构解析（默认 aarch64 / Apple Silicon）----
if [ "$#" -eq 0 ]; then
    ARCHES=(aarch64)
else
    ARCHES=("$@")
fi

# ---- 工具链选择 ----
# osxcross 的 clang 包装命名不唯一：可能是 <triple>-clang（aarch64-apple-darwin-clang）
# 或简写 oa64-clang/o64-clang，按可用性依次探测。
pick_tool() {
    local name="$1"; shift
    for cand in "$@"; do
        if command -v "$cand" >/dev/null 2>&1; then
            echo "$cand"
            return 0
        fi
    done
    echo "$name"  # 回退默认，让 cargo 报错提示
}

resolve_toolchain() {
    local arch="$1"
    case "$arch" in
        aarch64)
            local cc
            cc="$(pick_tool aarch64-apple-darwin-clang oa64-clang)"
            cat <<EOF
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$cc"
export CC_aarch64_apple_darwin="$cc"
export CXX_aarch64_apple_darwin="$(pick_tool "${cc}++" oa64-clang++)"
$(if command -v aarch64-apple-darwin-ar >/dev/null 2>&1; then echo 'export CARGO_TARGET_AARCH64_APPLE_DARWIN_AR="aarch64-apple-darwin-ar"'; fi)
export TARGET="aarch64-apple-darwin"
export PLATFORM_DIR="darwin-aarch64"
EOF
            ;;
        x86_64)
            local cc
            cc="$(pick_tool x86_64-apple-darwin-clang o64-clang)"
            cat <<EOF
export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="$cc"
export CC_x86_64_apple_darwin="$cc"
export CXX_x86_64_apple_darwin="$(pick_tool "${cc}++" o64-clang++)"
$(if command -v x86_64-apple-darwin-ar >/dev/null 2>&1; then echo 'export CARGO_TARGET_X86_64_APPLE_DARWIN_AR="x86_64-apple-darwin-ar"'; fi)
export TARGET="x86_64-apple-darwin"
export PLATFORM_DIR="darwin-x86_64"
EOF
            ;;
        *) echo "[ERROR] unsupported arch: $arch" >&2; exit 1 ;;
    esac
}

# Rust 源目录：部分模块在 src/rust，其余在 src/main/rust
rust_src_dir() {
    local module_name="$1"
    if [ -f "$RUST_SRC_BASE/utils-support-native-${module_name}/src/rust/Cargo.toml" ]; then
        echo "$RUST_SRC_BASE/utils-support-native-${module_name}/src/rust"
    else
        echo "$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/rust"
    fi
}

# ---- Rust cdylib 构建 ----
build_rust_module() {
    local module_name="$1"
    local lib_name="$2"
    local target="$3"
    local platform_dir="$4"

    local rust_dir
    rust_dir="$(rust_src_dir "$module_name")"
    local output_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/resources/native/${platform_dir}"

    if [ ! -f "${rust_dir}/Cargo.toml" ]; then
        echo "[SKIP] ${module_name}: no Cargo.toml at ${rust_dir}"
        return 0
    fi

    echo "[BUILD] ${module_name} -> ${platform_dir}/lib${lib_name}.dylib"
    mkdir -p "${output_dir}"

    export RUSTFLAGS="--cfg macos -C link-arg=-Wl,-rpath,@loader_path"

    if cargo build \
        --manifest-path "${rust_dir}/Cargo.toml" \
        --target "$target" \
        --release \
        --target-dir "${CARGO_TARGET_DIR}" \
        -q 2>&1; then
        :
    else
        echo "[FAIL] ${module_name} build failed"
        return 1
    fi

    local dylib="${CARGO_TARGET_DIR}/${target}/release/lib${lib_name}.dylib"
    if [ -f "${dylib}" ]; then
        cp "${dylib}" "${output_dir}/lib${lib_name}.dylib"
        chmod 755 "${output_dir}/lib${lib_name}.dylib"
        echo "[OK]   ${module_name}: lib${lib_name}.dylib ($(stat -c%s "${output_dir}/lib${lib_name}.dylib") bytes)"
    else
        echo "[FAIL] ${module_name}: lib not found at ${dylib}"
        return 1
    fi
}

# ---- C 模块（sqlite hook）构建 ----
build_c_module() {
    local module_name="$1"
    local target="$2"
    local platform_dir="$3"
    local arch="$4"

    local c_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/c"
    local output_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/resources/native/${platform_dir}"

    if [ ! -f "${c_dir}/sqlite3_hook_macos.c" ]; then
        echo "[SKIP] ${module_name}: no sqlite3_hook_macos.c"
        return 0
    fi

    echo "[BUILD-C] ${module_name} -> ${platform_dir}/libsqlite3_hook.dylib"
    mkdir -p "${output_dir}"

    case "$arch" in
        aarch64) local cc="$(pick_tool aarch64-apple-darwin-clang oa64-clang)" ;;
        x86_64)  local cc="$(pick_tool x86_64-apple-darwin-clang o64-clang)" ;;
    esac

    if ! "$cc" -O2 -dynamiclib -fPIC \
        -I"${c_dir}" \
        -o "${output_dir}/libsqlite3_hook.dylib" \
        "${c_dir}/sqlite3_hook_macos.c" \
        -lpthread 2>&1; then
        echo "[WARN] ${module_name} C build failed"
        return 0
    fi

    if [ -f "${output_dir}/libsqlite3_hook.dylib" ]; then
        echo "[OK]   ${module_name}: libsqlite3_hook.dylib ($(stat -c%s "${output_dir}/libsqlite3_hook.dylib") bytes)"
    else
        echo "[FAIL] ${module_name}: .dylib not produced"
    fi
}

echo "============================================================"
echo " Native macOS Build (osxcross)"
echo " OSXCROSS_ROOT: ${OSXCROSS_ROOT}"
echo " JNI Headers:   ${JNI_HEADERS}"
echo " Arch:          ${ARCHES[*]}"
echo "============================================================"
echo ""

for arch in "${ARCHES[@]}"; do
    # eval 使 resolve_toolchain 输出的 export 在当前 shell 生效
    eval "$(resolve_toolchain "$arch")"
    target="$TARGET"
    platform_dir="$PLATFORM_DIR"

    echo ""
    echo "========== Building for ${target} (${platform_dir}) =========="

    rustup target add "$target" 2>/dev/null || true
    # --- JNI 模块 ---
    build_rust_module "video-codec"      "chua_native_video_codec" "$target" "$platform_dir" || true
    build_rust_module "nmap"             "rust_nmap"               "$target" "$platform_dir" || true
    build_rust_module "datarecovery"     "data_recovery_ffi"       "$target" "$platform_dir" || true
    build_rust_module "video-processor"  "video_processor"         "$target" "$platform_dir" || true

    # --- C-style Rust 模块 ---
    build_rust_module "filesearch"       "file_search"             "$target" "$platform_dir" || true
    build_rust_module "headless"         "headless_rust"           "$target" "$platform_dir" || true
    build_rust_module "filestorage"      "file_storage"            "$target" "$platform_dir" || true
    build_rust_module "smb"              "rust_smb_server"         "$target" "$platform_dir" || true
    build_rust_module "ffmpeg"           "ffmpeg_rust"             "$target" "$platform_dir" || true
    build_rust_module "metrics"          "metrics_native"          "$target" "$platform_dir" || true

    # --- C 模块 ---
    build_c_module "sqlite" "$target" "$platform_dir" "$arch" || true
done

echo ""
echo "============================================================"
echo " Build complete. macOS .dylib files:"
echo "============================================================"
find "$RUST_SRC_BASE" -path "*/native/darwin-*/*.dylib" -exec ls -la {} \; 2>/dev/null
echo "============================================================"