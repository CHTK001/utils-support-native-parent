"""Quantize MiniMind ONNX to under 100MB for GitHub.

Tries dynamic quantization QInt8 first, then QInt4 if still too big.
Input:  D:\\ch\\project\\minimind-3\\model.onnx  (FP16 from convert_to_onnx.py)
Output: D:\\ch\\project\\minimind-3\\model-int8.onnx
"""

import os
import sys

from onnxruntime.quantization import QuantType, quantize_dynamic

SRC = r"D:\ch\project\minimind-3\model.onnx"
DST = r"D:\ch\project\minimind-3\model-int8.onnx"


def main() -> int:
    if not os.path.isfile(SRC):
        print(f"ERROR: not found {SRC}", file=sys.stderr)
        return 1

    src_size = os.path.getsize(SRC) / 1024 / 1024
    print(f"==> Input: {SRC} ({src_size:.1f}MB)")

    if os.path.isfile(DST):
        os.remove(DST)

    # Step 1: QInt8 dynamic quantization (typically 3-4x smaller)
    print(f"==> QInt8 dynamic quantization...")
    quantize_dynamic(
        model_input=SRC,
        model_output=DST,
        weight_type=QuantType.QInt8,
        op_types_to_quantize=["MatMul", "Gemm"],
        extra_options={"MatMulConstBOnly": False},
    )
    dst_size = os.path.getsize(DST) / 1024 / 1024
    print(f"    QInt8: {dst_size:.1f}MB")
    if dst_size < 100:
        print(f"==> Done: {src_size:.1f}MB -> {dst_size:.1f}MB (under 100MB)")
        return 0

    # Step 2: QInt4 if still too large
    print(f"==> Still {dst_size:.1f}MB, trying QInt4...")
    quantize_dynamic(
        model_input=SRC,
        model_output=DST,
        weight_type=QuantType.QInt4,
        op_types_to_quantize=["MatMul", "Gemm"],
        extra_options={"MatMulConstBOnly": False},
    )
    dst_size = os.path.getsize(DST) / 1024 / 1024
    print(f"    QInt4: {dst_size:.1f}MB")
    print(f"==> Final: {src_size:.1f}MB -> {dst_size:.1f}MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
