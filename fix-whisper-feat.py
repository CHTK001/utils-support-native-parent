import os, json, shutil

DEST = r'D:\ch\project\whisper-tiny-feat2'
SRC = r'D:\ch\project\utils-support-models-parent\utils-support-models-onnx-whisper\src\main\resources\audio\asr\whisper-tiny'

# Write proper preprocessor_config.json (whisper specific)
with open(os.path.join(DEST, 'preprocessor_config.json'), 'w', encoding='utf-8') as f:
    json.dump({
        'feature_extractor_type': 'WhisperFeatureExtractor',
        'feature_size': 80,
        'hop_length': 160,
        'chunk_length': 30,
        'n_fft': 400,
        'n_samples': 480000,
        'nb_max_frames': 3000,
        'padding': 'longest',
        'padding_side': 'right',
        'return_attention_mask': False,
        'sampling_rate': 16000
    }, f, indent=2)

# copy other files
for n in ['tokenizer_config.json', 'vocab.json', 'special_tokens_map.json']:
    src = os.path.join(SRC, n)
    dst = os.path.join(DEST, n)
    if not os.path.exists(dst) and os.path.exists(src):
        shutil.copy(src, dst)

# tokenizer_config.json was overwritten by HF one — good. But special_tokens_map.json needed.
print(os.listdir(DEST))