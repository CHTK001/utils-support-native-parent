"""WS bridge - robust frame collector, then full guac SSH flow"""
import socket, json, sys, struct, os, urllib.request, time

sys.stdout.reconfigure(encoding='utf-8', errors='replace')

def authenticate(protocol='SSH', host='127.0.0.1', port=22, user='root', password='rootpass123'):
    body = json.dumps({'mode':'custom','protocol':protocol,'host':host,'port':port,
                       'user':user,'password':password}).encode()
    req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate',
                                 data=body, headers={'Content-Type':'application/json'})
    return json.loads(urllib.request.urlopen(req, timeout=10).read().decode())

def ws_connect(ws_path):
    key = 'dGhlIHNhbXBsZSBub25jZQ=='
    http_req = (f'GET {ws_path} HTTP/1.1\r\n'
                f'Host: 172.16.0.40:18182\r\n'
                f'Connection: Upgrade\r\nUpgrade: websocket\r\n'
                f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n').encode()
    s = socket.socket()
    s.settimeout(20)
    s.connect(('172.16.0.40', 18182))
    s.send(http_req)
    resp = b''
    while b'\r\n\r\n' not in resp:
        resp += s.recv(4096)
    return s, resp.split(b'\r\n')[0].decode()

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

class WSReader:
    def __init__(self, sock):
        self.sock = sock
        self.buf = b''
    def read_exact(self, n):
        while len(self.buf) < n:
            c = self.sock.recv(n - len(self.buf))
            if not c:
                raise EOFError('connection closed')
            self.buf += c
        d = self.buf[:n]
        self.buf = self.buf[n:]
        return d
    def recv_frame(self, timeout=5):
        self.sock.settimeout(timeout)
        try:
            hdr = self.read_exact(2)
            op = hdr[0] & 0x0F
            length = hdr[1] & 0x7F
            if length == 126:
                length = struct.unpack('>H', self.read_exact(2))[0]
            elif length == 127:
                length = struct.unpack('>Q', self.read_exact(8))[0]
            data = self.read_exact(length)
            return op, data
        except (EOFError, socket.timeout):
            return None, b''

print("=== 1. authenticate ===")
result = authenticate()
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f"  tunnel: {tunnel_id}")

print("\n=== 2. WS connect ===")
s, hs = ws_connect(ws_path)
print(f"  handshake: {hs}")
reader = WSReader(s)

print("\n=== 3. collect all frames (bound + args) ===")
all_text = b''
for i in range(10):
    op, data = reader.recv_frame(timeout=3)
    if op is None:
        print(f"  frame {i}: timeout")
        break
    print(f"  frame {i}: op=0x{op:x} len={len(data)}")
    if op in (0x1, 0x2):
        all_text += data
    elif op == 0x8:
        print(f"  close: {data[:60]}")
        break

print(f"\n  collected: {len(all_text)} bytes")
print(f"  {all_text[:500]!r}")

# Parse guacamole instructions
print("\n=== parse instructions ===")
inst_list = []
pos = 0
while pos < len(all_text):
    sc = all_text.find(b';', pos)
    if sc < 0:
        break
    seg = all_text[pos:sc]
    pos = sc + 1
    if seg:
        inst_list.append(seg)
for i, s in enumerate(inst_list):
    print(f"  instr {i}: {s[:200]!r}")

# Now if we have args, proceed with size+connect
has_args = any(seg.startswith(b'4.args') or b'args' in seg for seg in inst_list)
if has_args:
    print("\n=== 4. send size + connect ===")
    send_ws(s, 0x2, b'4.size,4.1000,4.750,1.96;')
    time.sleep(0.3)
    def enc(v):
        return str(len(v.encode())).encode() + b'.' + v.encode() + b','
    def inst(*parts):
        sb = b''
        for p in parts:
            sb += enc(p)
        return sb[:-1] + b';'
    connect_parts = ['connect', '1000', '750', '96',
        '127.0.0.1', '', '22', 'root', 'rootpass123',
        'monospace', '12', 'true', '/', 'false', 'false',
        '', '', 'en_US', '', '', 'false', '', '',
        'false', 'false', 'false', 'false', 'false', '0', '',
        'linux', '10000', 'en_US', '', 'false', 'false', 'false',
        '', '', '', '0', '0']
    send_ws(s, 0x2, inst(*connect_parts))
    print("  sent size + connect")

    print("\n=== 5. read SSH output ===")
    all_out = b''
    start = time.time()
    while time.time() - start < 12:
        op, data = reader.recv_frame(timeout=3)
        if op is None: continue
        if op in (0x1, 0x2):
            all_out += data
            print(f"  got op=0x{op:x} len={len(data)}: {data[:150]!r}")
        elif op == 0x8:
            print(f"  close {data[:60]}")
            break
    print(f"\n  total SSH data: {len(all_out)} bytes")
    print(all_out[:800])
s.close()
print("\n=== DONE ===")