"""Check guacd modules - is SSH compiled in?"""
import requests, socket, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=30):
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

# Check guacd dynamic libraries (which protocols are compiled in)
out, err = docker_exec_sync(["sh", "-c", "ldd /usr/local/sbin/guacd 2>&1 | head -30"], timeout=10)
print("=== ldd guacd ===")
print(out)

print("\n=== find SSH-related libs ===")
out, err = docker_exec_sync(["sh", "-c", "find / -name 'libguac-client*' 2>/dev/null | head"], timeout=10)
print(out)

print("\n=== /usr/local/lib ===")
out, err = docker_exec_sync(["sh", "-c", "ls -la /usr/local/lib/ 2>&1 | head -20"], timeout=10)
print(out)

print("\n=== protocol list ===")
out, err = docker_exec_sync(["sh", "-c", "ls /usr/local/lib/libguac-client-*.so 2>&1"], timeout=10)
print(out)

print("\n=== strace guacd briefly ===")
out, err = docker_exec_sync(["sh", "-c", "timeout 2 /usr/local/sbin/guacd -L 2>&1 | head -30"], timeout=10)
print(out)

print("\n=== guacd -h ===")
out, err = docker_exec_sync(["sh", "-c", "/usr/local/sbin/guacd --help 2>&1"], timeout=10)
print(out)
