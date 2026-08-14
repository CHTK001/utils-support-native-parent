"""Fix libguac.so.24 symlink and restart guacd"""
import requests, socket, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=60):
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
        except Exception:
            break
    s.close()
    parts = buf.split(b'\r\n\r\n', 1)
    body_buf = parts[1] if len(parts) > 1 else b''
    pos = 0; stdout = ''; stderr = ''
    while pos + 8 <= len(body_buf):
        hdr = body_buf[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        chunk = body_buf[pos:pos+size]; pos += size
        text = chunk.decode('utf-8', errors='replace')
        if stream == 1: stdout += text
        elif stream == 2: stderr += text
    return stdout, stderr

# Check what libguac.so files exist
print("=== /usr/local/lib libguac* ===")
out, err = docker_exec_sync(["sh", "-c", "ls -la /usr/local/lib/libguac* 2>&1"], timeout=10)
print(out)

print("\n=== ldconfig ===")
out, err = docker_exec_sync(["sh", "-c", "ldconfig 2>&1; echo EXIT=$?"], timeout=20)
print(out)

print("\n=== after ldconfig, find libguac.so ===")
out, err = docker_exec_sync(["sh", "-c", "ls -la /usr/local/lib/libguac* 2>&1"], timeout=10)
print(out)

print("\n=== test guacd start ===")
out, err = docker_exec_sync(["sh", "-c", "ldd /usr/local/sbin/guacd 2>&1 | grep -i 'not found'"], timeout=10)
print("missing libs:", out)

print("\n=== manual symlink ===")
out, err = docker_exec_sync(["sh", "-c",
    "ls /usr/local/lib/libguac.so* 2>&1; "
    "ln -sf /usr/local/lib/libguac.so /usr/local/lib/libguac.so.24 2>&1; "
    "ln -sf /usr/local/lib/libguac.so /usr/local/lib/libguac.so.24.0.0 2>&1; "
    "ls -la /usr/local/lib/libguac.so* 2>&1; "
    "ldconfig 2>&1; echo DONE"], timeout=20)
print(out)

print("\n=== ldd again ===")
out, err = docker_exec_sync(["sh", "-c", "ldd /usr/local/sbin/guacd 2>&1 | grep -iE 'not found|libguac'"], timeout=10)
print(out)
