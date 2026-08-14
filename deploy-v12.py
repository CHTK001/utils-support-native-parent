"""Deploy gateway jar to container via Docker API 2375 + restart"""
import requests, tarfile, io, sys, time, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"
JAR_LOCAL = r"D:\ch\project\gateway-server-v13.jar"

def docker_exec(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True}, timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    import socket
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
            if not c:
                break
            buf += c
            if b'\r\n\r\n' in buf and len(buf) > 200:
                break
        except Exception:
            break
    s.close()
    parts = buf.split(b'\r\n\r\n', 1)
    body = parts[1] if len(parts) > 1 else b''
    pos = 0
    out = ''
    while pos + 8 <= len(body):
        hdr = body[pos:pos+8]; pos += 8
        stream = hdr[0]; size = int.from_bytes(hdr[4:8], 'big')
        if size > 1024 * 1024:
            break
        chunk = body[pos:pos+size]; pos += size
        if stream in (1, 2):
            out += chunk.decode('utf-8', errors='replace')
    return out

print("=== 1. inspect ===")
data = requests.get(f"{DOCKER}/containers/{CONTAINER}/json", timeout=10).json()
print(f"  state: {data['State']['Status']}")
print(f"  pid: {data['State']['Pid']}")

print("\n=== 2. upload jar (tar PUT to /app) ===")
tar_buf = io.BytesIO()
with tarfile.open(fileobj=tar_buf, mode='w') as t:
    t.add(JAR_LOCAL, arcname='gateway-server.jar')
tar_buf.seek(0)
print(f"  tar size: {len(tar_buf.getvalue())} bytes")
r = requests.put(f"{DOCKER}/containers/{CONTAINER}/archive?path=/app",
                 data=tar_buf.getvalue(), timeout=600,
                 headers={'Content-Type': 'application/x-tar'})
print(f"  PUT status: {r.status_code}, body: {r.text[:200]}")

print("\n=== 3. verify jar size ===")
out = docker_exec(["ls", "-la", "/app/gateway-server.jar"])
print(out)

print("\n=== 4. restart container ===")
r = requests.post(f"{DOCKER}/containers/{CONTAINER}/restart", timeout=30)
print(f"  restart status: {r.status_code}")

print("\n=== 5. wait for API ===")
for i in range(10):
    time.sleep(5)
    try:
        r = requests.get(f"http://172.16.0.40:18090/api/connections/list", timeout=3)
        print(f"  attempt {i+1}: status={r.status_code} body={r.text[:100]}")
        if r.status_code == 200:
            break
    except Exception as e:
        print(f"  attempt {i+1}: {e}")
