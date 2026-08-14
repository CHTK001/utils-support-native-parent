"""Recompile guacamole-server with -Wno-error=discarded-qualifiers"""
import requests, socket, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=300):
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

# Recompile with warning disabled
# CFLAGS=-Wno-error add to make
print("=== make with -Wno-error=discarded-qualifiers ===")
out, err = docker_exec_sync(
    ["sh", "-c",
     "cd /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5 && "
     "make distclean 2>&1 | tail -2 && "
     "CFLAGS='-O2 -Wno-error=discarded-qualifiers -Wno-discarded-qualifiers' ./configure --enable-ssh --enable-vnc 2>&1 | tail -15 && "
     "make -j4 2>&1 | tail -20"],
    timeout=600)
print("STDOUT (tail):", out[-4000:])
print("STDERR:", err[:500])