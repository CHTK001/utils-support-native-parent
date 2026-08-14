"""Bare WS client for v12: connect -> read bound -> read guacd args -> send size -> check output"""
import socket, json, sys, struct, os, urllib.request, time

sys.stdout.reconfigure(encoding='utf-8', errors='replace')

print("=== 1. authenticate (SSH guac) ===")
body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate', data=body, headers={'Content-Type':'application/json'})
result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f"  tunnel: {tunnel_id}")
print(f"  ws_path: {ws_path}")

print("\n=== 2. WS handshake ===")
key = 'dGhlIHNhbXBsZSBub25jZQ=='
http_req = (
    f'GET {ws_path} HTTP/1.1\r\n'
    f'Host: 172.16.0.40:18182\r\n'
    f'Connection: Upgrade\r\nUpgrade: websocket\r\n'
    f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n'
).encode()
s = socket.socket()
s.settimeout(15)
s.connect(('172.16.0.40', 18182))
s.send(http_req)
resp = b''
while b'\r\n\r\n' not in resp:
    resp += s.recv(4096)
line = resp.split(b'\r\n')[0].decode()
print(f"  handshake: {line}")

def send_ws(sock, op, data, mask=True):
    if mask:
        m = os.urandom(4)
        masked = bytes(b ^ m[i % 4] for i, b in enumerate(data))
        if len(data) < 126:
            hdr = bytes([0x80 | op, 0x80 | len(data)])
        else:
            hdr = bytes([0x80 | op, 0x80 | 126]) + struct.pack('>H', len(data))
        sock.send(hdr + m + masked)
    else:
        if len(data) < 126:
            hdr = bytes([0x80 | op, len(data)])
        else:
            hdr = bytes([0x80 | op, 126]) + struct.pack('>H', len(data))
        sock.send(hdr + data)

def recv_ws(sock, timeout=5):
    sock.settimeout(timeout)
    try:
        hdr = b''
        while len(hdr) < 2:
            c = sock.recv(2 - len(hdr))
            if not c: return None, None, b''
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
    except Exception as e:
        return None, None, b''

print("\n=== 3. read frames (bound then guacd args) ===")
frames = []
start = time.time()
while time.time() - start < 10:
    fin, op, data = recv_ws(s, timeout=2)
    if op is None: continue
    frames.append((op, data))
    print(f"  op=0x{op:x} len={len(data)} data={data[:200]!r}")
    if op == 0x8:
        print(f"  close {int.from_bytes(data[:2],'big')} {data[2:].decode(errors='replace')}")
        break
    if len(frames) >= 3:
        break

# If we got args, send size (guacamole-common-js sends size after args)
print("\n=== 4. send size (if got args) ===")
has_args = any(b'args' in d for op, d in frames if op == 0x2)
if has_args:
    inst = "4.size,4.1000,4.750,1.96;"
    send_ws(s, 0x2, inst.encode('utf-8'))
    print(f"  sent size: {inst}")

    # Then send connect
    inst2 = "7.connect;"
    send_ws(s, 0x2, inst2.encode('utf-8'))
    print(f"  sent connect: {inst2}")

print("\n=== 5. read more frames (SSH output) ===")
all_bytes = b''
start = time.time()
while time.time() - start < 12:
    fin, op, data = recv_ws(s, timeout=3)
    if op is None: continue
    if op in (0x1, 0x2):
        all_bytes += data
        print(f"  got op=0x{op:x} len={len(data)}: {data[:200]!r}")
    elif op == 0x8:
        print(f"  close {int.from_bytes(data[:2],'big')} {data[2:].decode(errors='replace')}")
        break

print(f"\n=== 6. total bytes: {len(all_bytes)} ===")
print(all_bytes.decode('utf-8', errors='replace')[:1000])
s.close()
print("\n=== DONE ===")
