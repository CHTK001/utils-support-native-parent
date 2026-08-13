"""Raw WS dump"""
import socket, json, sys, struct, os, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import urllib.request

print('=== auth ===')
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
s.settimeout(10)
s.connect(('172.16.0.40', 18182))
s.send(http_req)
resp = b''
while b'\r\n\r\n' not in resp:
    resp += s.recv(4096)
print('handshake:', resp.split(b'\r\n')[0].decode())

# read frames for 10s
for _ in range(20):
    try:
        hdr = s.recv(2)
        if not hdr: break
        op = hdr[0] & 0x0F
        length = hdr[1] & 0x7F
        if length == 126: length = struct.unpack('>H', s.recv(2))[0]
        elif length == 127: length = struct.unpack('>Q', s.recv(8))[0]
        data = s.recv(length) if length else b''
        print(f'op={op:#x} len={length} data={data[:80]!r}')
        if op == 0x8: break
    except Exception as e:
        print('recv err:', e)
        break
s.close()
