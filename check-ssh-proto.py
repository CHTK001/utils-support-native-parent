"""Check make error and SSH protocol build state"""
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

# Check SSH protocol .o files
print("=== ssh protocol .o files ===")
out, err = docker_exec_sync(["sh", "-c", "find /root/.utils-support-gateway/cache/guacd/1.5.5 -path '*protocols/ssh*' -name '*.o' | head -20"], timeout=10)
print(out)

print("\n=== vnc protocol .o files ===")
out, err = docker_exec_sync(["sh", "-c", "find /root/.utils-support-gateway/cache/guacd/1.5.5 -path '*protocols/vnc*' -name '*.o' | head -20"], timeout=10)
print(out)

print("\n=== libguac-client .so files (all) ===")
out, err = docker_exec_sync(["sh", "-c", "find / -name 'libguac-client*' -o -name '*.so' 2>/dev/null | grep -i 'guac-client' | head -20"], timeout=15)
print(out)

print("\n=== config.log make error (tail) ===")
out, err = docker_exec_sync(["sh", "-c", "grep -i 'error\\|ssh\\|vnc' /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/config.log 2>&1 | tail -30"], timeout=10)
print(out)

print("\n=== make error location - libguac_la-string.lo ===")
out, err = docker_exec_sync(["sh", "-c", "ls -la /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/src/libguac/ | head -30"], timeout=10)
print(out)

print("\n=== check for libssh2 ===")
out, err = docker_exec_sync(["sh", "-c", "ls /usr/lib/x86_64-linux-gnu/libssh2* 2>&1"], timeout=10)
print(out)
