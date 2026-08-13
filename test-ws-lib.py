"""WS test using websocket-client lib (proper RFC 6455 decoder)"""
import json, sys, urllib.request, websocket, time

print("=== 1. authenticate ===")
body = json.dumps({'mode':'custom','protocol':'SSH','host':'127.0.0.1','port':22,'user':'root','password':'rootpass123'}).encode()
req = urllib.request.Request('http://172.16.0.40:18090/api/connections/authenticate', data=body, headers={'Content-Type':'application/json'})
result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
tunnel_id = result['data']['tunnelId']
ws_path = result['data']['wsUrl']
print(f'  tunnel: {tunnel_id}')
print(f'  ws_path: {ws_path}')

print("\n=== 2. WS connect ===")
ws_url = f"ws://172.16.0.40:18182{ws_path}"
ws = websocket.WebSocket()
ws.settimeout(20)
ws.connect(ws_url, header=[f"Host: 172.16.0.40:18182"])
print(f'  connected: {ws.connected}')

print("\n=== 3. recv first frame (bound) ===")
result = ws.recv()
print(f'  recv: {result[:200]!r}')
print(f'  json parsed: {json.loads(result)}')

print("\n=== 4. wait for SSH prompt (8s) ===")
ws.settimeout(8)
all_data = ''
try:
    while True:
        msg = ws.recv()
        if not msg:
            break
        all_data += msg
        print(f'  got {len(msg)}b chunk: {msg[:80]!r}')
except Exception as e:
    print(f'  recv done: {e}')

print(f'\n  total: {len(all_data)}b')
print(f'  end: {all_data[-300:]!r}')

print("\n=== 5. send 'id\\n' ===")
ws.send(b'id\n', opcode=websocket.ABNF.OPCODE_BINARY)
ws.settimeout(8)
out = ''
try:
    while True:
        msg = ws.recv()
        if not msg: break
        out += msg
        print(f'  got {len(msg)}b: {msg[:80]!r}')
except Exception as e:
    print(f'  recv done: {e}')

print(f'\n  total id output: {len(out)}b')
print(f'  parsed: {out!r}')

if 'uid=' in out:
    print('\n=== 6. SUCCESS ===')
else:
    print('\n=== 6. FAIL ===')

ws.close()
print("\n=== DONE ===")
