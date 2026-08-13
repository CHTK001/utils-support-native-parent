"""
v9 WS path 自动 bind + SSH 数据流测试
"""
import socket, json, sys, struct, os, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import urllib.request

print('=== 1. authenticate ===')
body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate', data=body, headers={'Content-Type':'application/json'})
result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f'  tunnel_id: {tunnel_id}')
print(f'  ws_path: {ws_path}')

print('\n=== 2. WS 连接（path 已含 tunnelId，无需 bind 帧）===')
key = 'dGhlIHNhbXBsZSBub25jZQ=='
http_req = (
    f'GET {ws_path} HTTP/1.1\r\n'
    f'Host: 172.16.0.40:18182\r\n'
    f'Connection: Upgrade\r\nUpgrade: websocket\r\n'
    f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n'
).encode()
s = socket.socket()
s.settimeout(30)
s.connect(('172.16.0.40', 18182))
s.send(http_req)
resp = b''
while b'\r\n\r\n' not in resp:
    resp += s.recv(4096)
h = resp.split(b'\r\n')[0].decode()
print(f'  handshake: {h}')

def send_ws(sock, op, data):
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    if len(data) < 126:
        hdr = bytes([0x80 | op, 0x80 | len(data)])
    else:
        hdr = bytes([0x80 | op, 0x80 | 126]) + struct.pack('>H', len(data))
    sock.send(hdr + mask + masked)

def recv_ws(sock, timeout=5):
    sock.settimeout(timeout)
    try:
        hdr = b''
        while len(hdr) < 2:
            c = sock.recv(2 - len(hdr))
            if not c: return None, None, None
            hdr += c
        op = hdr[0] & 0x0F
        length = hdr[1] & 0x7F
        if length == 126: length = struct.unpack('>H', sock.recv(2))[0]
        elif length == 127:
            length = 0
            for _ in range(8): length = (length << 8) | (sock.recv(1)[0])
        data = b''
        while len(data) < length:
            c = sock.recv(length - len(data))
            if not c: break
            data += c
        return None, op, data
    except Exception:
        return None, None, b''

# 读取 bound 确认（服务端会发 {"action":"bound"...}）
print('\n=== 3. 等 bound 确认 ===')
fin, op, data = recv_ws(s, timeout=5)
if op == 0x1:
    print(f'  server text: {data.decode()}')
elif op == 0x8:
    print(f'  server close: code={int.from_bytes(data[:2],"big")} reason={data[2:].decode("utf-8",errors="replace")}')
else:
    print(f'  op={op:#x} data={data[:100]!r}' if op is not None else '  op=None (no frame)')

# 读取 SSH 输出（prompt）
print('\n=== 4. 等 SSH prompt ===')
all_data = b''
start = time.time()
while time.time() - start < 8:
    fin, op, data = recv_ws(s, timeout=3)
    if op is None: continue
    if op == 0x2:
        all_data += data
        print(f'  bin: {len(data)}b head={data[:60]!r}')
    elif op == 0x1:
        all_data += data
        print(f'  text: {data[:100]!r}')
    elif op == 0x8:
        print(f'  close {int.from_bytes(data[:2],"big")} {data[2:].decode("utf-8",errors="replace")}')
        break

print(f'\n=== 5. prompt total: {len(all_data)} bytes ===')
try:
    print(all_data[:400].decode('utf-8', errors='replace'))
except: pass

# 发送 id 命令
print('\n=== 6. send id\\n ===')
send_ws(s, 0x2, b'id\n')

out = b''
start = time.time()
while time.time() - start < 8:
    fin, op, data = recv_ws(s, timeout=3)
    if op is None: continue
    if op == 0x2:
        out += data
        print(f'  bin: {len(data)}b {data[:100]!r}')
    elif op == 0x8:
        print(f'  close {int.from_bytes(data[:2],"big")} {data[2:].decode("utf-8",errors="replace")}')
        break

print(f'\n=== 7. id output: {len(out)} bytes ===')
try:
    print(out[:400].decode('utf-8', errors='replace'))
except: pass

# 发送 pwd
print('\n=== 8. send pwd\\n ===')
send_ws(s, 0x2, b'pwd\n')
out2 = b''
start = time.time()
while time.time() - start < 8:
    fin, op, data = recv_ws(s, timeout=3)
    if op is None: continue
    if op == 0x2:
        out2 += data
        print(f'  bin: {len(data)}b {data[:100]!r}')
    elif op == 0x8:
        print(f'  close {int.from_bytes(data[:2],"big")} {data[2:].decode("utf-8",errors="replace")}')
        break

print(f'\n=== 9. pwd output: {len(out2)} bytes ===')
try:
    print(out2[:400].decode('utf-8', errors='replace'))
except: pass

s.close()
print('\n=== DONE ===')