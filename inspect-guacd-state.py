"""Inspect existing compile state in container"""
import requests, socket, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=120):
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

# Check what's already compiled
print("=== existing .o files ===")
out, err = docker_exec_sync(["sh", "-c", "find /root/.utils-support-gateway/cache/guacd/1.5.5 -name '*.o' 2>/dev/null | wc -l"], timeout=30)
print(out)

print("\n=== src/guacd/*.o ===")
out, err = docker_exec_sync(["sh", "-c", "ls /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/src/guacd/*.o 2>&1 | head -20"], timeout=10)
print(out)

print("\n=== src/protocols ===")
out, err = docker_exec_sync(["sh", "-c", "ls /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/src/protocols/ 2>&1"], timeout=10)
print(out)

print("\n=== src/common *.o ===")
out, err = docker_exec_sync(["sh", "-c", "find /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/src -name '*.o' 2>&1 | wc -l"], timeout=10)
print(out)

print("\n=== guacd binary stat ===")
out, err = docker_exec_sync(["sh", "-c", "stat /usr/local/sbin/guacd && md5sum /usr/local/sbin/guacd"], timeout=10)
print(out)

print("\n=== apt available ===")
out, err = docker_exec_sync(["sh", "-c", "apt-cache search guacd 2>&1 | head; apt-cache search libfreerdp 2>&1 | head"], timeout=15)
print(out)

print("\n=== installed dev libs ===")
out, err = docker_exec_sync(["sh", "-c", "dpkg -l 2>&1 | grep -E 'libssh|libvnc|libpango|libfreerdp|libcairo' | head"], timeout=10)
print(out)
