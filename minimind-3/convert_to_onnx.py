"""Convert minimind-3 (HuggingFace safetensors) to ONNX with inline weights.

Uses minimind's own MiniMindForCausalLM class (D:\\ch\\project\\minimind\\model\\
model_minimind.py) which has a simple forward that doesn't create DynamicCache.

Output: D:\\ch\\project\\minimind-3\\model.onnx (fp32, opset 14, dynamic batch
+ dynamic sequence, single file with weights inlined).
"""

import math
import os
import sys

import torch
from safetensors import safe_open

SRC = r"D:\ch\project\minimind-3"
OUT = os.path.join(SRC, "model.onnx")
sys.path.insert(0, r"D:\ch\project\minimind")
from model.model_minimind import MiniMindConfig, MiniMindForCausalLM  # noqa: E402


def load_config() -> MiniMindConfig:
    return MiniMindConfig(
        hidden_size=768,
        num_hidden_layers=8,
        num_attention_heads=8,
        num_key_value_heads=4,
        head_dim=96,
        hidden_act="silu",
        intermediate_size=2432,
        max_position_embeddings=32768,
        rms_norm_eps=1e-6,
        rope_theta=1_000_000.0,
        vocab_size=6400,
        tie_word_embeddings=True,
        flash_attn=False,
        use_moe=False,
        dropout=0.0,
    )


def load_state_dict(path: str) -> dict:
    state = {}
    with safe_open(path, framework="pt") as f:
        for k in f.keys():
            t = f.get_tensor(k).to(torch.float32)
            state[k] = t
    return state


class ExportWrapper(torch.nn.Module):
    """Strip GenerationMixin so torch.onnx.export sees a clean forward."""

    def __init__(self, model: MiniMindForCausalLM):
        super().__init__()
        self._m = model

    def forward(self, input_ids: torch.Tensor) -> torch.Tensor:
        out = self._m.forward(input_ids, use_cache=False, return_dict=True)
        return out.logits


def main() -> int:
    print("==> Building MiniMindForCausalLM (default config)")
    config = load_config()
    model = MiniMindForCausalLM(config)
    model.eval()

    print("==> Loading safetensors into model (FP32 for ONNX quantize compatibility)")
    state = load_state_dict(os.path.join(SRC, "model.safetensors"))
    missing, unexpected = model.load_state_dict(state, strict=False)
    print(f"missing keys   : {len(missing)}  {missing[:3]}")
    print(f"unexpected keys: {len(unexpected)}  {unexpected[:3]}")
    assert not unexpected, f"unexpected keys: {unexpected[:3]}"
    real_missing = [k for k in missing if k != "lm_head.weight"]
    assert not real_missing, f"real missing keys: {real_missing[:3]}"

    wrapper = ExportWrapper(model)
    wrapper.eval()

    print("==> Sanity-check forward (batch=1, seq=8)")
    with torch.no_grad():
        out = wrapper(torch.tensor([[1, 2, 3, 4, 5, 6, 7, 8]], dtype=torch.long))
    print(f"  logits shape={tuple(out.shape)} dtype={out.dtype}")
    top5 = torch.topk(out[0, -1], 5).indices.tolist()
    print(f"  next-token top5 ids={top5}")

    print(f"==> Exporting ONNX to {OUT}")
    dummy = torch.zeros(1, 8, dtype=torch.long)
    with torch.no_grad():
        torch.onnx.export(
            wrapper,
            (dummy,),
            OUT,
            input_names=["input_ids"],
            output_names=["logits"],
            dynamic_axes={
                "input_ids": {0: "batch", 1: "sequence"},
                "logits": {0: "batch", 1: "sequence"},
            },
            opset_version=14,
            do_constant_folding=True,
            dynamo=False,
        )
    print(f"==> ONNX written: {os.path.getsize(OUT)/1024/1024:.1f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
