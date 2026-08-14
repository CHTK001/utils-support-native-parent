"""Direct guacd test in container via Docker exec"""
import requests, socket, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True, "Tty": False},
                       timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    s = socket.create_connection(("172.16.0.40", 2375), timeout=timeout)
    req = (f'POST /exec/{cid}/start HTTP/1.1\r\nHost: 172.16.0.40:2375\r\n'
           f'Content-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n'
           f'Content-Length: 2\r\n\r\n{{}}').encode()
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
    pos = 0
    out = ''
    while pos + 8 <= len(buf):
        hdr = buf[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        if size > 1024*1024: break
        chunk = buf[pos:pos+size]; pos += size
        if stream in (1, 2):
            out += chunk.decode('utf-8', errors='replace')
    return out

print("=== ps aux ===")
out = docker_exec(["sh", "-c", "ps aux | grep -E 'guac|java' | head -20"], timeout=10)
print(out)

print("\n=== netstat listening ===")
out = docker_exec(["sh", "-c", "netstat -tlnp 2>/dev/null || ss -tlnp"], timeout=10)
print(out)

print("\n=== guacd binary ===")
out = docker_exec(["sh", "-c", "ls -la /usr/local/sbin/guacd /usr/local/bin/guacd 2>&1"], timeout=10)
print(out)

print("\n=== guacd version ===")
out = docker_exec(["sh", "-c", "/usr/local/sbin/guacd -V 2>&1 || /usr/local/bin/guacd -V 2>&1"], timeout=10)
print(out)

print("\n=== test guacd via python in container ===")
out = docker_exec(["sh", "-c", "echo '6.select,3.ssh,9.127.0.0.1,2.22,4.root,11.rootpass123,4.1024,3.768,2.96;' | timeout 3 nc -w 2 127.0.0.1 4822 2>&1 | head -20 || echo NC_NOT_FOUND"], timeout=10)
print(out)

print("\n=== check which nc ===")
out = docker_exec(["sh", "-c", "which nc python3 python 2>&1"], timeout=10)
print(out)
