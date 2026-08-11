"""Generate 5s 'real' speech-like audio with formant structure + spectral tilt."""
import os
import struct
import wave
import math
import numpy as np


def gen_speech(dur=10.0, sr=16000):
    """Generate speech-like signal: time-varying formant synthesis + breath noise."""
    t = np.linspace(0, dur, int(sr * dur), endpoint=False)
    # 4 formants (Hz), vary slowly with smooth transitions
    base = np.array([400.0, 1700.0, 2500.0, 3500.0])
    # formant modulation: slowly change formants 200Hz over time
    f1 = base[0] + 100 * np.sin(2 * math.pi * 0.3 * t)
    f2 = base[1] + 200 * np.sin(2 * math.pi * 0.5 * t + 1)
    f3 = base[2] + 300 * np.sin(2 * math.pi * 0.4 * t + 2)
    f4 = base[3] + 100 * np.sin(2 * math.pi * 0.7 * t + 3)
    # Source: glottal pulse train (50% duty)
    pitch = 130 + 30 * np.sin(2 * math.pi * 0.2 * t)
    pulse = (np.sin(2 * math.pi * np.cumsum(pitch / sr)) > 0.3).astype(np.float32)
    # Filter (formants applied via simple additive synthesis)
    audio = (np.sin(2 * math.pi * np.cumsum(f1 / sr) * t)
             + np.sin(2 * math.pi * np.cumsum(f2 / sr) * t) * 0.7
             + np.sin(2 * math.pi * np.cumsum(f3 / sr) * t) * 0.5
             + np.sin(2 * math.pi * np.cumsum(f4 / sr) * t) * 0.3) * pulse * 0.3
    # breath noise
    audio += 0.05 * np.random.randn(len(audio))
    # syllable envelope
    syllable = (np.sin(2 * math.pi * 4 * t) > 0).astype(np.float32)
    audio = audio * (0.5 + 0.5 * syllable)
    # normalize
    audio = audio / (np.abs(audio).max() + 1e-8) * 0.8
    audio = audio.astype(np.float32)
    path = os.path.join(os.environ.get("TEMP", "/tmp"), "whisper_long.wav")
    with wave.open(path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sr)
        for s in audio:
            wf.writeframesraw(struct.pack("<h", int(s * 32767)))
    return path


if __name__ == "__main__":
    p = gen_speech()
    print(p)