"""
Full WS SSH session: connect, send command, read output
"""
import socket, json, sys, struct, os
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# 1. Authenticate
print('=== 1. authenticate ===')
import urllib.request
body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate',
    data=body, headers={'Content-Type':'application/json'})
result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f'  tunnel_id: {tunnel_id}')

# 2. WS handshake
print('\n=== 2. WS handshake ===')
key = 'dGhlIHNhbXBsZSBub25jZQ=='
http_req = (
    f'GET {ws_path} HTTP/1.1\r\n'
    f'Host: 172.16.0.40:18182\r\n'
    f'Connection: Upgrade\r\n'
    f'Upgrade: websocket\r\n'
    f'Sec-WebSocket-Key: {key}\r\n'
    f'Sec-WebSocket-Version: 13\r\n'
    f'\r\n'
).encode()
s = socket.socket()
s.settimeout(10)
s.connect(('172.16.0.40', 18182))
s.send(http_req)
resp = b''
while b'\r\n\r\n' not in resp:
    chunk = s.recv(4096)
    if not chunk: break
    resp += chunk
status_line = resp.split(b'\r\n')[0].decode()
print(f'  status: {status_line}')
assert '101' in status_line

# 3. Send WS frame (text "id\n")
def send_ws_text(sock, text):
    data = text.encode()
    # client→server must be masked
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    length = len(data)
    if length < 126:
        header = bytes([0x81, 0x80 | length])
    elif length < 65536:
        header = bytes([0x81, 0x80 | 126]) + struct.pack('>H', length)
    else:
        header = bytes([0x81, 0x80 | 127]) + struct.pack('>Q', length)
    sock.send(header + mask + masked)

def recv_ws_frame(sock, timeout=5):
    sock.settimeout(timeout)
    try:
        hdr = b''
        while len(hdr) < 2:
            chunk = sock.recv(2 - len(hdr))
            if not chunk: return None, None, b''
            hdr += chunk
        fin = hdr[0] & 0x80
        op = hdr[0] & 0x0F
        masked = hdr[1] & 0x80
        length = hdr[1] & 0x7F
        if length == 126:
            length = struct.unpack('>H', sock.recv(2))[0]
        elif length == 127:
            length = struct.unpack('>Q', sock.recv(8))[0]
        if masked:
            mask = sock.recv(4)
        else:
            mask = b''
        data = b''
        while len(data) < length:
            chunk = sock.recv(length - len(data))
            if not chunk: break
            data += chunk
        if masked and mask:
            data = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        return fin, op, data
    except Exception as e:
        return None, None, b''

# Send "id\n"
print('\n=== 3. send "id\\n" ===')
send_ws_text(s, 'id\n')

# Read response frames
print('\n=== 4. recv output ===')
all_data = b''
import time as t
start = t.time()
while t.time() - start < 5:
    fin, op, data = recv_ws_frame(s, timeout=2)
    if op is None: break
    if op == 0x1:  # text
        all_data += data
        print(f'  text frame: {data!r}')
    elif op == 0x2:  # binary
        all_data += data
        print(f'  binary frame: {data[:100]!r}...')
    elif op == 0x8:  # close
        print('  close')
        break
    elif op == 0x9:  # ping
        print('  ping')
    else:
        print(f'  op={op:#x} data={data!r}')

print(f'\n=== 5. total output: {len(all_data)} bytes ===')
print(all_data[:500].decode('utf-8', errors='replace'))

s.close()
print('\n=== DONE ===')