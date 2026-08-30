#!/usr/bin/env bash
# ============================================================================
# CUDA 运行时库一键准备脚本（Linux）
# 自动: 探测驱动 CUDA 版本 -> 读取 scripts/cuda.env 配置 -> pip 下载 nvidia-*-cu12
#       -> 拷贝 .so 到 TARGET_DIR -> 校验关键库 -> 可选 ldconfig
# 用法: setup-cuda.sh [目标目录] [CUDA主版本]
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${SCRIPT_DIR}/cuda.env"
ROOT="$(dirname "${SCRIPT_DIR}")"

# ---------- 读取 cuda.env 配置（禁止硬编码） ----------
if [[ -f "${ENV_FILE}" ]]; then
    # shellcheck disable=SC1090
    source "${ENV_FILE}"
fi

TARGET="${1:-${TARGET_DIR:-libs/cuda}}"
CUDA_VER="${2:-${CUDA_MAJOR:-12}}"
FULL_TARGET="${ROOT}/${TARGET}"
mkdir -p "${FULL_TARGET}"

echo "===================================================="
echo " CUDA 运行库准备 (Linux, CUDA ${CUDA_VER})"
echo " 目标目录: ${FULL_TARGET}"
echo "===================================================="

# ---------- 1. 探测 NVIDIA 驱动 ----------
if ! command -v nvidia-smi >/dev/null 2>&1; then
    echo "[ERROR] 未检测到 NVIDIA 驱动，请先安装: sudo apt-get install nvidia-driver"
    exit 1
fi
DRIVER="$(nvidia-smi --query-gpu=driver_version --format=csv,noheader | head -1)"
echo "  驱动版本: ${DRIVER}"

# ---------- 2. 关键库已存在则跳过下载 ----------
NEED=0
[[ -f "${FULL_TARGET}/libcudart.so.${CUDA_VER}" ]] || NEED=1
[[ -f "${FULL_TARGET}/libcublas.so.${CUDA_VER}" ]] || NEED=1
[[ -f "${FULL_TARGET}/libcudnn.so.9" ]] || NEED=1
if [[ "${NEED}" == "0" ]]; then
    echo "[SKIP] 关键库已就绪，无需下载"
    SKIP_DL=1
fi

# ---------- 3. venv 隔离安装 nvidia 运行时包 ----------
if [[ "${NEED}" == "1" ]]; then
    VENV="${ROOT}/target/cuda-venv"
    if [[ ! -x "${VENV}/bin/python" ]]; then
        python3 -m venv "${VENV}"
    fi
    PIP="${VENV}/bin/pip"
    if [[ ! -x "${PIP}" ]]; then
        echo "[ERROR] venv 创建失败，请确认 python3 可用"
        exit 1
    fi
    for pkg in "${PIP_RUNTIME:-nvidia-cuda-runtime-cu${CUDA_VER}}" \
               "${PIP_CUBLAS:-nvidia-cublas-cu${CUDA_VER}}" \
               "${PIP_CUDNN:-nvidia-cudnn-cu${CUDA_VER}}"; do
        echo "  pip install ${pkg} ..."
        "${PIP}" install --quiet --disable-pip-version-check "${pkg}" || \
            echo "[WARN] ${pkg} 安装失败"
    done

    # ---------- 4. 拷贝 .so 到目标目录 ----------
    echo "  拷贝 .so 到 ${FULL_TARGET} ..."
    find "${VENV}/lib" -name "*.so*" -type f -exec cp -f {} "${FULL_TARGET}/" \; 2>/dev/null || true
fi

# ---------- 5. 校验 ----------
MISSING=""
[[ -f "${FULL_TARGET}/libcudart.so.${CUDA_VER}" ]] || MISSING="${MISSING} libcudart.so.${CUDA_VER}"
[[ -f "${FULL_TARGET}/libcublas.so.${CUDA_VER}" ]] || MISSING="${MISSING} libcublas.so.${CUDA_VER}"
[[ -f "${FULL_TARGET}/libcudnn.so.9" ]] || MISSING="${MISSING} libcudnn.so.9"
if [[ -n "${MISSING}" ]]; then
    echo "[ERROR] 缺失库:${MISSING}"
    exit 1
fi

# ---------- 6. 可选加入 ldconfig ----------
if [[ "${AUTO_PATH:-true}" == "true" ]]; then
    echo "  AUTO_PATH=true: 请将以下目录加入 ldconfig:"
    echo "    export LD_LIBRARY_PATH=\$LD_LIBRARY_PATH:${FULL_TARGET}"
fi

echo "[OK] CUDA 运行库就绪: ${FULL_TARGET}"
