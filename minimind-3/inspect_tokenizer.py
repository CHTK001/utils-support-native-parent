import json
import sys

with open(r'D:\ch\project\minimind-3\tokenizer.json', 'r', encoding='utf-8') as f:
    tk = json.load(f)

merges = tk['model'].get('merges', [])
print('merges type:', type(merges).__name__, 'len:', len(merges))
for i, m in enumerate(merges[:5]):
    sys.stdout.buffer.write(f'  merge[{i}]: {repr(m)}\n'.encode('utf-8'))

print('---vocab first 20---')
vocab = tk['model']['vocab']
for i, (k, v) in enumerate(vocab.items()):
    if i < 20:
        sys.stdout.buffer.write(f'  id={v} token={repr(k)}\n'.encode('utf-8'))
    else:
        break

print('---added_tokens (special)---')
for at in tk.get('added_tokens', [])[:10]:
    sys.stdout.buffer.write(f'  id={at["id"]} content={repr(at["content"])}\n'.encode('utf-8'))

print('---chat_template.jinja preview---')
import os
if os.path.exists(r'D:\ch\project\minimind-3\chat_template.jinja'):
    with open(r'D:\ch\project\minimind-3\chat_template.jinja', 'r', encoding='utf-8') as f:
        content = f.read()
        print(content[:1500])
