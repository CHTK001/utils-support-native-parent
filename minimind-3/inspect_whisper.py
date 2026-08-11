"""Inspect Whisper-tiny ONNX encoder + decoder input/output signatures."""

import onnxruntime as ort
import sys

ENCODER = r"D:\ch\project\utils-support-models-parent\utils-support-models-onnx-whisper\src\main\resources\audio\asr\whisper-tiny\onnx\encoder_model_quantized.onnx"
DECODER = r"D:\ch\project\utils-support-models-parent\utils-support-models-onnx-whisper\src\main\resources\audio\asr\whisper-tiny\onnx\decoder_model_merged_quantized.onnx"

for label, path in [("ENCODER", ENCODER), ("DECODER", DECODER)]:
    print(f"=== {label}: {path} ===")
    sess = ort.InferenceSession(path, providers=["CPUExecutionProvider"])
    print("Inputs:")
    for inp in sess.get_inputs():
        print(f"  {inp.name:30s} shape={inp.shape!s:30s} type={inp.type}")
    print("Outputs:")
    for out in sess.get_outputs():
        print(f"  {out.name:30s} shape={out.shape!s:30s} type={out.type}")
    print()
