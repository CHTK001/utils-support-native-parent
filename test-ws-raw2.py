"""Raw dump of all bytes received"""
import socket, json, sys, struct, os, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import urllib.request

body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate', data=body, headers={'Content-Type':'application/json'})
result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f'tunnel={tunnel_id} path={ws_path}')

key = 'dGhlIHNhbXBsZSBub25jZQ=='
http_req = (
    f'GET {ws_path} HTTP/1.1\r\n'
    f'Host: 172.16.0.40:18182\r\n'
    f'Connection: Upgrade\r\nUpgrade: websocket\r\n'
    f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n'
).encode()
s = socket.socket()
s.settimeout(20)
s.connect(('172.16.0.40', 18182))
s.send(http_req)

# Handshake response
resp = b''
while b'\r\n\r\n' not in resp:
    resp += s.recv(4096)
print('handshake:', resp.split(b'\r\n')[0].decode())

# Try to read first WS frame - dump all bytes raw
s.settimeout(5)
try:
    raw = s.recv(2)
    print(f'first 2 bytes: {raw.hex()} ascii={raw!r}')
    if len(raw) >= 2:
        b1 = raw[0]; b2 = raw[1]
        print(f'  b1=0x{b1:02x} (FIN={b1>>7} RSV={(b1>>4)&0x7} op={b1&0xF:04x})')
        print(f'  b2=0x{b2:02x} (MASK={b2>>7} len={b2&0x7F})')
        length = b2 & 0x7F
        if length == 126:
            extra = s.recv(2)
            length = struct.unpack('>H', extra)[0]
            print(f'  extended len: {length}')
        elif length == 127:
            extra = s.recv(8)
            length = struct.unpack('>Q', extra)[0]
            print(f'  extended len: {length}')
        data = b''
        s.settimeout(10)
        while len(data) < length:
            chunk = s.recv(min(8192, length - len(data)))
            if not chunk: break
            data += chunk
        print(f'  data ({len(data)} bytes):')
        print(f'    hex: {data[:200].hex()}')
        print(f'    ascii: {data!r}')
except Exception as e:
    print('err:', e)
s.close()
