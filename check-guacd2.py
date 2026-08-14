"""Direct guacd test using Docker API with wait + sync result"""
import requests, socket, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=30):
    """Create exec, start with Detach=false, read all, return (stdout, stderr, exit_code)"""
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True, "Tty": False},
                       timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    s = socket.create_connection(("172.16.0.40", 2375), timeout=timeout)
    body = json.dumps({"Detach": False}).encode()
    req = (f'POST /exec/{cid}/start HTTP/1.1\r\nHost: 172.16.0.40:2375\r\n'
           f'Content-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n'
           f'Content-Length: {len(body)}\r\n\r\n').encode() + body
    s.sendall(req)
    s.settimeout(timeout)
    buf = b''
    while True:
        try:
            c = s.recv(8192)
            if not c: break
            buf += c
            if b'\r\n\r\n' in buf and len(buf) > 8 + len(b'\r\n\r\n'):
                # Continue reading body
                pass
        except Exception:
            break
    s.close()
    # Skip HTTP headers
    parts = buf.split(b'\r\n\r\n', 1)
    body_buf = parts[1] if len(parts) > 1 else b''
    stdout = ''
    stderr = ''
    pos = 0
    while pos + 8 <= len(body_buf):
        hdr = body_buf[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        chunk = body_buf[pos:pos+size]; pos += size
        text = chunk.decode('utf-8', errors='replace')
        if stream == 1:
            stdout += text
        elif stream == 2:
            stderr += text
    return stdout, stderr

print("=== ps aux ===")
out, err = docker_exec_sync(["sh", "-c", "ps aux 2>&1 | grep -E 'guac|java' | head -10"], timeout=10)
print("STDOUT:", out)
print("STDERR:", err)

print("\n=== ss / netstat listening ports ===")
out, err = docker_exec_sync(["sh", "-c", "ss -tlnp 2>&1 | head -20"], timeout=10)
print("STDOUT:", out)
print("STDERR:", err)

print("\n=== guacd files ===")
out, err = docker_exec_sync(["sh", "-c", "ls -la /usr/local/sbin/guacd 2>&1"], timeout=10)
print("STDOUT:", out)

print("\n=== Test guacd connect: send select via python in container ===")
# Use python in container to test guacd locally
test_script = """
python3 -c "
import socket
s = socket.socket()
s.settimeout(3)
try:
    s.connect(('127.0.0.1', 4822))
    print('CONNECTED to guacd')
    inst = b'6.select,3.ssh,9.127.0.0.1,2.22,4.root,11.rootpass123,4.1024,3.768,2.96;'
    s.sendall(inst)
    print('SENT select')
    data = s.recv(4096)
    print('RECV:', data[:500])
except Exception as e:
    print('ERROR:', e)
" 2>&1
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=15)
print("STDOUT:", out)
print("STDERR:", err)
