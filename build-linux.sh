#!/usr/bin/env bash
# ============================================================
# utils-support-native-parent ? Linux x86_64 ????
# ? Linux ???????
#   chmod +x build-linux.sh && ./build-linux.sh
# ???rustup, cargo, x86_64-linux-gnu-gcc (?????)
# ??????apt install gcc-multilib g++-multilib
# ============================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_SRC_BASE="$SCRIPT_DIR"
JNI_HEADERS="$SCRIPT_DIR/jni-headers/linux"
CARGO_TARGET_DIR="$SCRIPT_DIR/target-linux"
OUTPUT_BASE="$SCRIPT_DIR"

# ?? Linux ?????
rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true

# ?? Rust ?????C-style ???
build_rust_module() {
    local module_name="$1"
    local rust_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/rust"
    local output_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/resources/native/linux-x86_64"
    local lib_name="$2"

    if [ ! -f "${rust_dir}/Cargo.toml" ]; then
        echo "[SKIP] ${module_name}: no Cargo.toml"
        return 0
    fi

    echo "[BUILD] ${module_name} -> ${lib_name}.so"
    mkdir -p "${output_dir}"

    export RUSTFLAGS="--cfg linux"

    cargo build \
        --manifest-path "${rust_dir}/Cargo.toml" \
        --target x86_64-unknown-linux-gnu \
        --release \
        --target-dir "${CARGO_TARGET_DIR}" \
        -q 2>&1 || {
            echo "[FAIL] ${module_name} build failed"
            return 1
        }

    local dll="${CARGO_TARGET_DIR}/x86_64-unknown-linux-gnu/release/lib${lib_name}.so"
    if [ -f "${dll}" ]; then
        cp "${dll}" "${output_dir}/${lib_name}.so"
        chmod 755 "${output_dir}/${lib_name}.so"
        echo "[OK]   ${module_name}: ${lib_name}.so $(stat -c%s "${output_dir}/${lib_name}.so") bytes"
    else
        echo "[FAIL] ${module_name}: lib not found at ${dll}"
        return 1
    fi
}

# JNI ????? JNI ????
build_jni_module() {
    local module_name="$1"
    local rust_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/rust"
    local output_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/resources/native/linux-x86_64"
    local lib_name="$2"

    if [ ! -f "${rust_dir}/Cargo.toml" ]; then
        echo "[SKIP] ${module_name}: no Cargo.toml"
        return 0
    fi

    echo "[BUILD-JNI] ${module_name} -> ${lib_name}.so"
    mkdir -p "${output_dir}"

    # JNI ?????
    export JAVA_INCLUDE="-I${JNI_HEADERS}"
    export BINDGEN_EXTRA_CLANG_ARGS="--target=x86_64-unknown-linux-gnu"

    cargo build \
        --manifest-path "${rust_dir}/Cargo.toml" \
        --target x86_64-unknown-linux-gnu \
        --release \
        --target-dir "${CARGO_TARGET_DIR}" \
        -q 2>&1 || {
            echo "[WARN] ${module_name} may need JAVA_HOME set for JNI headers"
            return 1
        }

    local dll="${CARGO_TARGET_DIR}/x86_64-unknown-linux-gnu/release/lib${lib_name}.so"
    if [ -f "${dll}" ]; then
        cp "${dll}" "${output_dir}/${lib_name}.so"
        chmod 755 "${output_dir}/${lib_name}.so"
        echo "[OK]   ${module_name}: ${lib_name}.so $(stat -c%s "${output_dir}/${lib_name}.so") bytes"
    else
        echo "[FAIL] ${module_name}: lib not found"
        return 1
    fi
}

# C ???????sqlite?
build_c_module() {
    local module_name="$1"
    local c_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/c"
    local output_dir="$RUST_SRC_BASE/utils-support-native-${module_name}/src/main/resources/native/linux-x86_64"
    local lib_name="$2"

    if [ ! -f "${c_dir}/${lib_name}.c" ]; then
        echo "[SKIP] ${module_name}: no C source"
        return 0
    fi

    echo "[BUILD-C] ${module_name} -> ${lib_name}.so"
    mkdir -p "${output_dir}"

    gcc -shared -fPIC -O2 -o "${output_dir}/${lib_name}.so" \
        "${c_dir}/${lib_name}.c" \
        -I"${JNI_HEADERS}" \
        -Wl,-soname,${lib_name}.so \
        -Wl,-rpath,'$ORIGIN' 2>&1 || {
            echo "[WARN] ${module_name} C build may need adjustment"
            return 0
        }

    if [ -f "${output_dir}/${lib_name}.so" ]; then
        echo "[OK]   ${module_name}: ${lib_name}.so $(stat -c%s "${output_dir}/${lib_name}.so") bytes"
    else
        echo "[FAIL] ${module_name}: .so not produced"
    fi
}

echo "============================================================"
echo " Native Linux Build ? x86_64-unknown-linux-gnu"
echo " JAVA_HOME: ${JAVA_HOME:-not set}"
echo " JNI Headers: ${JNI_HEADERS}"
echo " Target: x86_64-unknown-linux-gnu"
echo "============================================================"
echo ""

# --- JNI ?? ---
build_jni_module "video-codec"  "chua_native_video_codec"
build_jni_module "nmap"          "rust_nmap"
build_jni_module "datarecovery"  "data_recovery_ffi"
build_jni_module "video-processor" "video_processor"

# --- C-style Rust ?? ---
build_rust_module "filesearch"   "file_search"
build_rust_module "headless"     "headless_rust"
build_rust_module "filestorage"  "file_storage"
build_rust_module "smb"          "rust_smb_server"
build_rust_module "ffmpeg"       "ffmpeg_rust"
build_rust_module "metrics"      "metrics_native"

# --- C ???? ---
build_c_module "sqlite" "sqlite3_hook"

echo ""
echo "============================================================"
echo " Build complete. Linux .so files:"
echo "============================================================"
find "$RUST_SRC_BASE" -path "*/native/linux-x86_64/*.so" -exec ls -la {} \; 2>/dev/null
echo "============================================================"
