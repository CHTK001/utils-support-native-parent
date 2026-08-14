"""通过 Docker API 部署 v11 gateway jar：
  - PUT /containers/{id}/archive?path=/app  (tar stream upload)
  - kill java to trigger container restart
"""
import requests, tarfile, io, sys, time, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"
JAR_LOCAL = r"D:\ch\project\gateway-server-v11.jar"

print("=== 1. inspect ===")
data = requests.get(f"{DOCKER}/containers/{CONTAINER}/json", timeout=10).json()
print(f"  state: {data['State']['Status']}")
print(f"  java pid: {data['State']['Pid']}")
print(f"  restart policy: {data['HostConfig'].get('RestartPolicy')}")

print("\n=== 2. tar + PUT to /containers/{id}/archive?path=/app ===")
tar_buf = io.BytesIO()
with tarfile.open(fileobj=tar_buf, mode='w') as t:
    t.add(JAR_LOCAL, arcname='gateway-server.jar')
tar_buf.seek(0)
print(f"  tar size: {len(tar_buf.getvalue())} bytes")

r = requests.put(
    f"{DOCKER}/containers/{CONTAINER}/archive?path=/app",
    data=tar_buf.getvalue(),
    timeout=600,
    headers={'Content-Type': 'application/x-tar'}
)
print(f"  PUT status: {r.status_code}, body: {r.text[:200]}")

if r.status_code != 200:
    print("  upload FAILED")
    sys.exit(1)

print("  upload OK")

print("\n=== 3. verify via /containers/{id}/json exec ===")
# Use exec via http.client for hijacked stdout
import http.client

def exec_run(cmd, timeout=30):
    cr = requests.post(
        f"{DOCKER}/containers/{CONTAINER}/exec",
        json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True},
        timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    conn = http.client.HTTPConnection("172.16.0.40", 2375, timeout=timeout)
    conn.request("POST", f"/exec/{cid}/start", body=json.dumps({}).encode(), headers={
        "Content-Type": "application/json",
        "Connection": "Upgrade",
        "Upgrade": "tcp"
    })
    resp = conn.getresponse()
    out = b''
    try:
        while True:
            chunk = resp.read(8192)
            if not chunk:
                break
            out += chunk
    finally:
        conn.close()
    # Docker stream wraps stdout/stderr with a 8-byte header per frame: stream_type(1) + padding(3) + size(4)
    # Decode frames
    pos = 0
    stdout = []
    while pos + 8 <= len(out):
        hdr = out[pos:pos+8]
        pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        chunk = out[pos:pos+size]
        pos += size
        if stream in (1, 2):
            stdout.append(chunk.decode('utf-8', errors='replace'))
    return ''.join(stdout)

print("  ls -la /app/:")
print(exec_run(["ls", "-la", "/app/"]))

print("\n=== 4. kill java to trigger container restart ===")
# Wait for restart, then check
print("  killing java...")
exec_run(["sh", "-c", "kill -9 $(pgrep -f gateway-server.jar) ; sleep 1 ; ps -ef | grep -E 'java|gateway' | head -10"])

print("\n=== 5. wait for gateway restart (10s) ===")
time.sleep(10)
out = exec_run(["ps", "-ef"])
print(out[:1500])

print("\n=== 6. check API ===")
for i in range(5):
    try:
        r = requests.get(f"http://172.16.0.40:18090/api/connections/list", timeout=3)
        print(f"  attempt {i+1}: status={r.status_code} body={r.text[:200]}")
        if r.status_code == 200:
            break
    except Exception as e:
        print(f"  attempt {i+1}: {e}")
    time.sleep(3)
