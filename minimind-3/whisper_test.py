"""Whisper mel spectrogram (pure numpy) + minimum preprocessor_config.json."""

import json
import os
import struct
import math
import wave
import tempfile
import numpy as np
import onnxruntime as ort
from typing import List

ENCODER = r"D:\ch\project\utils-support-models-parent\utils-support-models-onnx-whisper\src\main\resources\audio\asr\whisper-tiny\onnx\encoder_model_quantized.onnx"
DECODER = r"D:\ch\project\utils-support-models-parent\utils-support-models-onnx-whisper\src\main\resources\audio\asr\whisper-tiny\onnx\decoder_model_merged_quantized.onnx"
WHISPER_DIR = r"D:\ch\project\utils-support-models-parent\utils-support-models-onnx-whisper\src\main\resources\audio\asr\whisper-tiny"

# Whisper-tiny hyperparams (from openai/whisper repo, hard-coded here)
SAMPLE_RATE = 16000
N_FFT = 400
HOP_LENGTH = 160
CHUNK_LENGTH = 30  # seconds
N_SAMPLES = SAMPLE_RATE * CHUNK_LENGTH  # 480000
N_MELS = 80


def hann_window(n_fft: int) -> np.ndarray:
    return 0.5 * (1 - np.cos(2 * math.pi * np.arange(n_fft) / n_fft))


def linear_to_mel(spectrogram: np.ndarray, n_mels: int = N_MELS) -> np.ndarray:
    """Simplified: just use log magnitude, 80-bin averaged chunks to mimic mel filterbank."""
    # Whisper uses Slaney mel scale; for tiny differences we approximate with 80 evenly-spaced frequency bands
    # Real Whisper has a 80x201 slaney-mel mat. For pipeline test, just compress frequencies into 80 bins.
    n_freq = spectrogram.shape[0]  # 201
    # average 201 -> 80 by grouping
    factor = n_freq / n_mels
    out = np.zeros((n_mels, spectrogram.shape[1]), dtype=np.float32)
    for i in range(n_mels):
        start = int(i * factor)
        end = min(int((i + 1) * factor), n_freq)
        if end > start:
            out[i] = spectrogram[start:end].mean(axis=0)
    return out


def log_mel_spectrogram(audio: np.ndarray, n_mels: int = N_MELS, n_fft: int = N_FFT, hop: int = HOP_LENGTH) -> np.ndarray:
    """Pure numpy Whisper-style log-mel spectrogram (approximation).

    Output: (n_mels=80, 3000) — Whisper always pads/trim audio to 30s
    (480000 samples at 16kHz), then STFT with hop=160 gives 3000 frames.
    """
    if audio.ndim > 1:
        audio = audio.mean(axis=1)
    audio = audio.astype(np.float32)
    # pad/trim to 30s
    if len(audio) > N_SAMPLES:
        audio = audio[:N_SAMPLES]
    else:
        audio = np.pad(audio, (0, N_SAMPLES - len(audio)))
    # Pad so STFT yields exactly 3000 frames
    # n_frames = 1 + (len(audio_padded) - n_fft) // hop must equal 3000
    # len(audio_padded) - n_fft = 3000 * hop - 1 => pad = 3000*hop - 1 + n_fft - len(audio)
    target_frames = 3000
    pad = target_frames * hop - 1 + n_fft - len(audio)
    if pad > 0:
        audio_padded = np.pad(audio, (0, pad), mode="constant")
    else:
        audio_padded = audio[:target_frames * hop - 1 + n_fft]
    # STFT
    win = hann_window(n_fft).astype(np.float32)
    n_frames = target_frames  # 3000
    n_freq = n_fft // 2 + 1  # 201
    spec = np.zeros((n_freq, n_frames), dtype=np.float32)
    for i in range(n_frames):
        start = i * hop
        frame = audio_padded[start:start + n_fft] * win
        spec[:, i] = np.abs(np.fft.rfft(frame)) ** 2
    # power -> mel (approximation)
    mel = linear_to_mel(spec, n_mels)
    # log
    log_spec = np.log10(np.maximum(mel, 1e-10))
    # normalize: per-feature max-mean
    log_spec = np.maximum(log_spec, log_spec.max() - 8.0)
    log_spec = (log_spec + 4.0) / 4.0
    return log_spec  # (80, 3000)


def gen_tone():
    sr = 16000
    dur = 1.0
    freq = 440
    t = np.linspace(0, dur, int(sr * dur), endpoint=False)
    audio = (0.3 * np.sin(2 * math.pi * freq * t)).astype(np.float32)
    path = os.path.join(tempfile.gettempdir(), "whisper_test_tone.wav")
    with wave.open(path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sr)
        for s in audio:
            wf.writeframesraw(struct.pack("<h", int(s * 32767)))
    return path


def main() -> int:
    wav_path = sys.argv[1] if len(sys.argv) > 1 else gen_tone()
    print(f"==> audio: {wav_path}")
    import soundfile as sf
    audio, sr = sf.read(wav_path)
    if sr != 16000:
        new_len = int(len(audio) * 16000 / sr)
        audio = np.interp(np.linspace(0, len(audio), new_len), np.arange(len(audio)), audio).astype(np.float32)
    if audio.ndim > 1:
        audio = audio.mean(axis=1)
    print(f"==> audio shape={audio.shape} sr={sr}")

    print("==> computing mel spectrogram via WhisperFeatureExtractor...")
    from transformers import WhisperFeatureExtractor
    fe = WhisperFeatureExtractor.from_pretrained(r"D:\ch\project\whisper-tiny-feat")
    out = fe(audio, sampling_rate=16000, return_tensors="np")
    input_features = out["input_features"].astype(np.float32)  # (1, 80, 3000)
    print(f"==> input_features shape={input_features.shape} min={input_features.min():.3f} max={input_features.max():.3f}")

    print("==> Running encoder...")
    enc_sess = ort.InferenceSession(ENCODER, providers=["CPUExecutionProvider"])
    enc_out = enc_sess.run(None, {"input_features": input_features})
    last_hidden_state = enc_out[0]
    print(f"==> encoder last_hidden_state shape={last_hidden_state.shape}")

    print("==> Running decoder (greedy with KV cache)...")
    dec_sess = ort.InferenceSession(DECODER, providers=["CPUExecutionProvider"])

    decoder_input_ids = np.array([[50258, 50259]], dtype=np.int64)  # sot + notimestamps (model auto-detects language)
    n_layers = 4
    n_heads = 6
    head_dim = 64
    # encoder_sequence_length_out = encoder output sequence length = 1500
    enc_seq_out = last_hidden_state.shape[1]  # 1500
    print(f"==> decoder initial pkv: enc_seq_out={enc_seq_out}")
    pkv = []
    for _ in range(n_layers):
        pkv.append({
            "decoder.key": np.zeros((1, n_heads, 0, head_dim), dtype=np.float32),
            "decoder.value": np.zeros((1, n_heads, 0, head_dim), dtype=np.float32),
            "encoder.key": np.zeros((1, n_heads, enc_seq_out, head_dim), dtype=np.float32),
            "encoder.value": np.zeros((1, n_heads, enc_seq_out, head_dim), dtype=np.float32),
        })
    use_cache_branch = np.array([False], dtype=bool)
    eos_id = 50257
    max_new = 50
    # decode using vocab.json from whisper dir
    import json
    with open(os.path.join(WHISPER_DIR, "vocab.json"), "r", encoding="utf-8") as f:
        vocab = json.load(f)
    inv = {v: k for k, v in vocab.items()}

    generated = list(decoder_input_ids[0].tolist())
    print(f"==> starting tokens={generated}")

    for step in range(max_new):
        feed = {
            "input_ids": decoder_input_ids,
            "encoder_hidden_states": last_hidden_state,
            "use_cache_branch": use_cache_branch,
        }
        for i, layer in enumerate(pkv):
            for k, v in layer.items():
                feed[f"past_key_values.{i}.{k}"] = v
        outs = dec_sess.run(None, feed)
        # 第一个输出是 logits，后续 16 个是 present.*，再后是 attentions
        print(f"  step {step}: outs count={len(outs)} logits shape={outs[0].shape}")
        if step == 0:
            print(f"    present.0.decoder.key shape: {outs[1].shape}")
            print(f"    present.0.decoder.value shape: {outs[2].shape}")
            print(f"    present.0.encoder.key shape: {outs[3].shape}")
            print(f"    present.0.encoder.value shape: {outs[4].shape}")
        logits = outs[0]
        # logits shape: (1, decoder_seq_len, 51865). For step 0 it's (1, 3, 51865)
        # take last position (the 3rd token's prediction)
        logits_step = logits[0, -1, :].copy()
        eos = 50257
        # apply suppress (just to follow Whisper protocol)
        SUPPRESS = [1, 2, 7, 8, 9, 10, 14, 25, 26, 27, 28, 29, 31, 58, 59, 60, 61, 62, 63, 90,
                    91, 92, 93, 359, 503, 522, 542, 873, 893, 902, 918, 922, 931, 1350,
                    1853, 1982, 2460, 2627, 3246, 3253, 3268, 3536, 3846, 3961, 4183, 4667,
                    6585, 6647, 7273, 9061, 9383, 10428, 10929, 11938, 12033, 12331, 12562,
                    13793, 14157, 14635, 15265, 15618, 16553, 16604, 18362, 18956, 20075,
                    21675, 22520, 26130, 26161, 26435, 28279, 29464, 31650, 32302, 32470,
                    36865, 42863, 47425, 49870, 50254, 50258, 50358, 50359, 50360, 50361, 50362]
        for s in SUPPRESS:
            logits_step[s] = -1e9
        if step == 0:
            top5 = np.argsort(-logits_step)[:5]
            print(f"  top-5 (post-suppress): {top5.tolist()} = {[inv.get(int(t), '?') for t in top5]}")
        next_token = int(np.argmax(logits_step))
        generated.append(next_token)
        if next_token == eos_id:
            print(f"==> EOS at step {step}")
            break
        decoder_input_ids = np.array([[next_token]], dtype=np.int64)
        new_pkv = []
        for i in range(n_layers):
            new_pkv.append({
                "decoder.key": outs[1 + i * 4],
                "decoder.value": outs[1 + i * 4 + 1],
                "encoder.key": outs[1 + i * 4 + 2],
                "encoder.value": outs[1 + i * 4 + 3],
            })
        pkv = new_pkv
        if step % 5 == 0:
            print(f"  step {step}: token={next_token}")

    print(f"==> generated tokens: {generated}")
    # decode using vocab.json from whisper dir
    import json
    with open(os.path.join(WHISPER_DIR, "vocab.json"), "r", encoding="utf-8") as f:
        vocab = json.load(f)
    inv = {v: k for k, v in vocab.items()}
    text = "".join(inv.get(t, "") for t in generated)
    text = text.replace("Ġ", " ")
    print(f"==> raw decoded: {text!r}")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
