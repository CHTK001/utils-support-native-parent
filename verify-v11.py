"""验证 v11 jar 已生效：看启动日志 + jar 大小 + guacd 进程"""
import requests, http.client, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def exec_run(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True}, timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    conn = http.client.HTTPConnection("172.16.0.40", 2375, timeout=timeout)
    conn.request("POST", f"/exec/{cid}/start", body=json.dumps({}).encode(), headers={
        "Content-Type": "application/json",
        "Connection": "Upgrade", "Upgrade": "tcp"
    })
    resp = conn.getresponse()
    out = b''
    try:
        while True:
            chunk = resp.read(8192)
            if not chunk: break
            out += chunk
    finally:
        conn.close()
    # Parse frames: 8-byte header stream_type(1)+pad(3)+size(4)
    pos = 0; stdout = []
    while pos + 8 <= len(out):
        hdr = out[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        chunk = out[pos:pos+size]; pos += size
        if stream in (1, 2):
            stdout.append(chunk.decode('utf-8', errors='replace'))
    return ''.join(stdout)

# Stream frame header is 8 bytes: stream type, padding, payload size
def exec_run_raw(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True}, timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    conn = http.client.HTTPConnection("172.16.0.40", 2375, timeout=timeout)
    conn.request("POST", f"/exec/{cid}/start", body=json.dumps({}).encode(), headers={
        "Content-Type": "application/json",
        "Connection": "Upgrade", "Upgrade": "tcp"
    })
    resp = conn.getresponse()
    out = b''
    try:
        while True:
            chunk = resp.read(8192)
            if not chunk: break
            out += chunk
    finally:
        conn.close()
    return out

def parse_frames(raw):
    pos = 0
    chunks = []
    while pos + 8 <= len(raw):
        hdr = raw[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        if size > 65536:
            # skip big chunk
            break
        chunk = raw[pos:pos+size]; pos += size
        chunks.append((stream, chunk))
    return chunks

print("=== 1. ls -la /app/ ===")
raw = exec_run_raw(["ls", "-la", "/app/"])
print(f"  raw len: {len(raw)}")
for stream, chunk in parse_frames(raw):
    label = "stdout" if stream == 1 else "stderr" if stream == 2 else "?"
    print(f"  [{label}] {chunk.decode('utf-8', errors='replace')}")

print("\n=== 2. jar content check ===")
raw = exec_run_raw(["sh", "-c", "cd /app && unzip -l gateway-server.jar | head -20"])
for stream, chunk in parse_frames(raw):
    label = "stdout" if stream == 1 else "stderr" if stream == 2 else "?"
    print(f"  [{label}] {chunk.decode('utf-8', errors='replace')}")

print("\n=== 3. check SshBridge presence (should NOT exist in v11) ===")
raw = exec_run_raw(["sh", "-c", "unzip -l /app/gateway-server.jar | grep -i 'SshBridge\\|jsch' || echo NOT_FOUND"])
for stream, chunk in parse_frames(raw):
    label = "stdout" if stream == 1 else "stderr" if stream == 2 else "?"
    print(f"  [{label}] {chunk.decode('utf-8', errors='replace')}")

print("\n=== 4. ps -ef ===")
raw = exec_run_raw(["ps", "-ef"])
for stream, chunk in parse_frames(raw):
    label = "stdout" if stream == 1 else "stderr" if stream == 2 else "?"
    print(f"  [{label}] {chunk.decode('utf-8', errors='replace')}")

print("\n=== 5. /api/connections/list (current API) ===")
r = requests.get(f"http://172.16.0.40:18090/api/connections/list", timeout=5)
print(f"  status={r.status_code} body={r.text}")
