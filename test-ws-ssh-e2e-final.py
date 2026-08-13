"""Clean v9 SSH WS E2E test with proper timing"""
import socket, json, sys, struct, os, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import urllib.request

print('=== 1. authenticate (SSH 127.0.0.1:22 root/rootpass123) ===')
body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate', data=body, headers={'Content-Type':'application/json'})
result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f'  tunnel_id: {tunnel_id}')
print(f'  ws_path:   {ws_path}')

print('\n=== 2. WS handshake ===')
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

def recv_ws(sock, timeout=10):
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
    except socket.timeout:
        return None, None, b''
    except Exception as e:
        return None, None, b''

# 1) Read bound frame (text 0x1)
print('\n=== 3. Wait for "bound" frame (text/JSON) ===')
start = time.time()
got_bound = False
while time.time() - start < 10 and not got_bound:
    _, op, data = recv_ws(s, timeout=2)
    if op == 0x1:
        print(f'  text: {data.decode(errors="replace")}')
        got_bound = True
    elif op == 0x2:
        print(f'  bin: {len(data)}b {data[:80]!r}')
    elif op == 0x8:
        print(f'  close {int.from_bytes(data[:2],"big")} {data[2:].decode(errors="replace")}')
        s.close()
        sys.exit(1)

if not got_bound:
    print('  !!! no bound frame within 10s')
    s.close()
    sys.exit(1)

# 2) Read SSH prompt + initial MOTD
print('\n=== 4. Wait for SSH prompt (e.g. "root@...#") up to 15s ===')
all_data = b''
start = time.time()
while time.time() - start < 15:
    _, op, data = recv_ws(s, timeout=2)
    if op is None or not data:
        if all_data and b'#' in all_data: break
        continue
    all_data += data
    if op == 0x2:
        # print only on new data
        sys.stdout.write('.')
        sys.stdout.flush()
    elif op == 0x1:
        sys.stdout.write('t')
        sys.stdout.flush()
    elif op == 0x8:
        print(f'\n  close {int.from_bytes(data[:2],"big")} {data[2:].decode(errors="replace")}')
        break

print(f'\n  total: {len(all_data)} bytes')
print(f'  ends with: {all_data[-200:]!r}')

# 3) Send "id" command
print('\n=== 5. send "id\\n" ===')
send_ws(s, 0x2, b'id\n')

start = time.time()
out = b''
while time.time() - start < 8:
    _, op, data = recv_ws(s, timeout=2)
    if op is None or not data: continue
    if op == 0x2:
        out += data
    elif op == 0x8:
        print(f'  close {int.from_bytes(data[:2],"big")} {data[2:].decode(errors="replace")}')
        break

print(f'  total: {len(out)} bytes')
print(f'  text: {out.decode(errors="replace")}')

# 4) Check "uid=" present
if b'uid=' in out:
    print('\n=== 6. SUCCESS: id command output received ===')
else:
    print('\n=== 6. FAIL: id command output not visible ===')

s.close()
print('\n=== DONE ===')
