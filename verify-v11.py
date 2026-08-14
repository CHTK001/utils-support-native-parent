"""Raw socket exec via Docker API"""
import requests, socket, json, time

DOCKER = 'http://172.16.0.40:2375'
CONTAINER = 'gateway-server'

def docker_exec(cmd):
    cr = requests.post(f'{DOCKER}/containers/{CONTAINER}/exec',
                       json={'Cmd': cmd, 'AttachStdout': True, 'AttachStderr': True}, timeout=10)
    cid = cr.json()['Id']
    s = socket.create_connection(('172.16.0.40', 2375), timeout=30)
    req = (
        f'POST /exec/{cid}/start HTTP/1.1\r\n'
        f'Host: 172.16.0.40:2375\r\n'
        f'Content-Type: application/json\r\n'
        f'Connection: Upgrade\r\nUpgrade: tcp\r\n'
        f'Content-Length: 2\r\n\r\n{{}}'
    ).encode()
    s.sendall(req)
    s.settimeout(10)
    chunks = []
    try:
        # First read until end of HTTP headers
        buf = b''
        while b'\r\n\r\n' not in buf:
            chunk = s.recv(4096)
            if not chunk:
                break
            buf += chunk
        # After headers, raw frames
        headers, _, body = buf.partition(b'\r\n\r\n')
        out = body
        while True:
            try:
                chunk = s.recv(65536)
                if not chunk:
                    break
                out += chunk
            except socket.timeout:
                break
    finally:
        s.close()
    # Skip HTTP headers
    parts = out.split(b'\r\n\r\n', 1)
    body = parts[1] if len(parts) > 1 else out
    # Parse 8-byte frames
    pos = 0; frames = []
    while pos + 8 <= len(body):
        hdr = body[pos:pos+8]; pos += 8
        size = int.from_bytes(hdr[4:8], 'big')
        if size > 1024 * 1024:
            break
        chunk = body[pos:pos+size]; pos += size
        frames.append((hdr[0], chunk))
    return frames

print('=== 1. inspect jar ===')
for stream, data in docker_exec(['sh', '-c', 'unzip -l /app/gateway-server.jar | grep -iE "SshBridge|jsch" || echo NOT_FOUND']):
    label = 'STDOUT' if stream == 1 else 'STDERR' if stream == 2 else '?'
    text = data.decode('utf-8', errors='replace')
    print(f'  [{label}] {text}')

print('\n=== 2. check GuacamoleBridge ===')
for stream, data in docker_exec(['sh', '-c', 'unzip -l /app/gateway-server.jar | grep -i "GuacamoleBridge" || echo NOT_FOUND']):
    label = 'STDOUT' if stream == 1 else 'STDERR' if stream == 2 else '?'
    print(f'  [{label}] {data.decode("utf-8", errors="replace")}')

print('\n=== 3. ls -la /app/ ===')
for stream, data in docker_exec(['ls', '-la', '/app/']):
    label = 'STDOUT' if stream == 1 else 'STDERR' if stream == 2 else '?'
    print(f'  [{label}] {data.decode("utf-8", errors="replace")}')

print('\n=== 4. ps -ef ===')
for stream, data in docker_exec(['ps', '-ef']):
    label = 'STDOUT' if stream == 1 else 'STDERR' if stream == 2 else '?'
    print(f'  [{label}] {data.decode("utf-8", errors="replace")}')

print('\n=== 5. API alive ===')
r = requests.get(f'http://172.16.0.40:18090/api/connections/list', timeout=5)
print(f'  status={r.status_code} body={r.text}')
