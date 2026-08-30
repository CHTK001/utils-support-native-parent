#!/usr/bin/env bash
# ============================================================================
# CUDA 运行库准备脚本（macOS）
# 说明: NVIDIA 已停止 macOS 平台 CUDA 支持（10.13 之后无新驱动），
#       Apple Silicon (M 系列) 无 NVIDIA GPU，此脚本仅做检测与提示。
# 用法: setup-cuda-macos.sh
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${SCRIPT_DIR}/cuda.env"
if [[ -f "${ENV_FILE}" ]]; then
    # shellcheck disable=SC1090
    source "${ENV_FILE}"
fi

echo "===================================================="
echo " CUDA 运行库准备 (macOS)"
echo "===================================================="

if [[ "$(uname -m)" == "arm64" ]]; then
    echo "[INFO] Apple Silicon (arm64): 无 NVIDIA GPU，CUDA 不可用。"
    echo "       如需 GPU 推理，请改用 Apple 的 Metal (CoreML) 或远程 GPU 服务。"
    exit 0
fi

if ! command -v nvidia-smi >/dev/null 2>&1; then
    echo "[INFO] 未检测到 NVIDIA 驱动。"
    echo "       NVIDIA 自 macOS 10.13 起停止提供 CUDA 驱动，本平台建议 CPU 推理。"
    exit 0
fi

echo "[WARN] 检测到 NVIDIA 驱动，但 macOS 上 onnxruntime-gpu 的 CUDA EP 不受官方支持，"
echo "       推荐使用 CPU 推理（onnxruntime CPU EP）。"
echo "[OK] 检查完成。"
