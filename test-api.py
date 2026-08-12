"""
Test directly via API
"""
import urllib.request, json, sys, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# 1. Test protocols
print('=== 1. protocols ===')
r = urllib.request.urlopen('http://172.16.0.40:18090/api/connections/list', timeout=5)
print(r.read().decode())

# 2. Test SSH auth (custom)
print('\n=== 2. SSH custom auth ===')
body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate',
    data=body, headers={'Content-Type':'application/json'})
r = urllib.request.urlopen(req, timeout=10)
result = json.loads(r.read().decode())
print(json.dumps(result, indent=2, ensure_ascii=False))

# 3. Test WS handshake
tunnel_id = result['data']['tunnelId']
print(f'\n=== 3. WS handshake for {tunnel_id} ===')
import socket
key = 'dGhlIHNhbXBsZSBub25jZQ=='
req = (
    f'GET /ws/SSH/{tunnel_id} HTTP/1.1\r\n'
    f'Host: 172.16.0.40:18182\r\n'
    f'Connection: Upgrade\r\n'
    f'Upgrade: websocket\r\n'
    f'Sec-WebSocket-Key: {key}\r\n'
    f'Sec-WebSocket-Version: 13\r\n'
    f'\r\n'
).encode()
s = socket.socket()
s.settimeout(5)
s.connect(('172.16.0.40', 18182))
s.send(req)
resp = b''
while b'\r\n\r\n' not in resp:
    resp += s.recv(4096)
print(resp.decode('utf-8', errors='replace').split('\r\n\r\n')[0])
s.close()