"""Full SSH flow: select -> size -> connect -> read streaming output"""
import requests, socket, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True, "Tty": False},
                       timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    s = socket.create_connection(("172.16.0.40", 2375), timeout=timeout)
    body = json.dumps({"Detach": False}).encode()
    req = (f'POST /exec/{cid}/start HTTP/1.1\r\nHost: 172.16.0.40:2375\r\n'
           f'Content-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n'
           f'Content-Length: {len(body)}\r\n\r\n').encode() + body
    s.sendall(req)
    s.settimeout(timeout)
    buf = b''
    while True:
        try:
            c = s.recv(8192)
            if not c: break
            buf += c
        except Exception:
            break
    s.close()
    parts = buf.split(b'\r\n\r\n', 1)
    body_buf = parts[1] if len(parts) > 1 else b''
    pos = 0; stdout = ''; stderr = ''
    while pos + 8 <= len(body_buf):
        hdr = body_buf[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        chunk = body_buf[pos:pos+size]; pos += size
        text = chunk.decode('utf-8', errors='replace')
        if stream == 1: stdout += text
        elif stream == 2: stderr += text
    return stdout, stderr

test_script = """
python3 << 'EOF'
import socket, time

def enc(v):
    return str(len(v.encode())).encode() + b'.' + v.encode() + b','

def inst(*parts):
    sb = b''
    for p in parts:
        sb += enc(p)
    return sb[:-1] + b';'

s = socket.socket()
s.settimeout(8)
s.connect(('127.0.0.1', 4822))

# 1. select
s.sendall(inst('select', 'ssh'))
args = s.recv(65536)
print('args:', args[:300])
time.sleep(0.2)

# 2. size
s.sendall(inst('size', '1024', '768', '96'))
time.sleep(0.2)

# 3. connect
connect_parts = ['connect', '1024', '768', '96',
    '127.0.0.1', '', '22', 'root', 'rootpass123',
    'monospace', '12', 'true', '/', 'false', 'false',
    '', '', 'en_US', '', '', 'false',
    '', '', 'false', 'false', 'false', 'false',
    'false', '0', '', 'linux', '10000', 'en_US', '',
    'false', 'false', 'false', '', '', '', '0', '0']
s.sendall(inst(*connect_parts))
time.sleep(1)
d = s.recv(65536)
print('ready:', d[:200])
time.sleep(0.5)

# 4. read SSH output (terminal stream: nop,pipe,...,ready,...)
all_data = b''
while True:
    try:
        chunk = s.recv(65536)
        if not chunk:
            break
        all_data += chunk
    except socket.timeout:
        break
    if len(all_data) > 5000:
        break

print()
print('=== SSH stream total:', len(all_data), 'bytes ===')
# In guacamole protocol, terminal data comes as "pipe,<stream-id>,<data>" instructions
# or raw bytes after connect. Let's dump parseable text.
# Find "pipe" instructions
pos = 0
text_parts = []
while pos < len(all_data):
    # parse guacamole instruction: len.value,len.value,...;
    sc = all_data.find(b';', pos)
    if sc < 0:
        break
    seg = all_data[pos:sc]
    pos = sc + 1
    if not seg:
        continue
    # decode elements
    elems = []
    i = 0
    while i < len(seg):
        dot = seg.find(b'.', i)
        if dot < 0:
            break
        ln = int(seg[i:dot])
        val = seg[dot+1:dot+1+ln]
        elems.append(val)
        i = dot + 1 + ln
        if seg[i:i+1] == b',':
            i += 1
    if elems and elems[0] in (b'pipe', b'error', b'clipboard', b'user'):
        text_parts.append((elems[0], elems[1] if len(elems)>1 else b'', elems[3] if len(elems)>3 else b''))
        if elems[0] == b'pipe':
            # pipe stream contains terminal data
            text_parts.append(b'STREAMDATA:' + (elems[3] if len(elems)>3 else b''))

print('Parsed instructions:')
for t in text_parts[:30]:
    if isinstance(t, tuple):
        print(f'  {t[0].decode()}: id={t[1][:40]!r} data={t[2][:100]!r}')
    else:
        print(f'  {t[:150]!r}')
s.close()
EOF
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=60)
print("STDOUT:")
print(out)
print("STDERR:")
print(err)