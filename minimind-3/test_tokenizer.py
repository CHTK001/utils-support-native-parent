import json
import sys
from transformers import AutoTokenizer

tk = AutoTokenizer.from_pretrained(r'D:\ch\project\minimind-3', trust_remote_code=False)
print('tokenizer loaded OK, vocab_size:', tk.vocab_size)

prompts = [
    '你好，请介绍一下自己。',
    '中国的首都是',
    '1+1=',
    '今天天气',
    '<|im_start|>system\n你是一个AI助手<|im_end|>\n<|im_start|>user\n你好<|im_end|>\n<|im_start|>assistant\n',
]
for p in prompts:
    ids = tk.encode(p, add_special_tokens=False)
    decoded = tk.decode(ids)
    sys.stdout.buffer.write(f'prompt: {p!r}\n'.encode('utf-8'))
    sys.stdout.buffer.write(f'  ids: {ids}\n'.encode('utf-8'))
    sys.stdout.buffer.write(f'  decoded: {decoded!r}\n'.encode('utf-8'))
    sys.stdout.buffer.write(f'  n_tokens: {len(ids)}\n'.encode('utf-8'))

# Apply chat template
messages = [
    {'role': 'system', 'content': '你是一个AI助手'},
    {'role': 'user', 'content': '你好'},
]
chat = tk.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
sys.stdout.buffer.write(f'\nchat_template output:\n{chat}\n'.encode('utf-8'))

chat_ids = tk.apply_chat_template(messages, tokenize=True, add_generation_prompt=True)
sys.stdout.buffer.write(f'chat_ids: {chat_ids}\n'.encode('utf-8'))
