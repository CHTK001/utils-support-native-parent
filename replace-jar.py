"""
Force replace the running jar inside the container
Use docker cp via tar: upload new gateway-server.jar to /app
"""
import urllib.request, json, time, sys, io, tarfile
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# Read local jar
with open(r"D:\ch\project\gateway-server.jar", "rb") as f:
    jar_bytes = f.read()
print(f"Local jar size: {len(jar_bytes)}")

# Make tar
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode='w') as tar:
    info = tarfile.TarInfo(name='gateway-server.jar')
    info.size = len(jar_bytes)
    tar.addfile(info, io.BytesIO(jar_bytes))
tar_data = buf.getvalue()
print(f"Tar size: {len(tar_data)}")

# Upload to /app
req = urllib.request.Request(f'{URL}/containers/gateway-server/archive?path=/app',
    data=tar_data,
    headers={'Content-Type': 'application/x-tar'},
    method='PUT')
r = urllib.request.urlopen(req, timeout=120)
print(f"Upload status: {r.status}")

# Now restart the container to pick up new jar
# First stop
try:
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/containers/gateway-server/stop?t=5', data=b'{}',
        headers={'Content-Type': 'application/json'}, method='POST')).read()
except: pass
time.sleep(5)

# Start
try:
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/containers/gateway-server/start',
        data=b'{}', headers={'Content-Type': 'application/json'},
        method='POST')).read()
    print("Started")
except Exception as e:
    print(f"Start err: {e}")

# Wait
time.sleep(10)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/json')
d = json.loads(r.read())
print(f"State: {d['State']['Status']} Running={d['State']['Running']} Pid={d['State']['Pid']}")

# Verify new jar inside
body = json.dumps({"Cmd": ["sh", "-c", "ls -la /app/gateway-server.jar > /tmp/p.log 2>&1; sha256sum /app/gateway-server.jar >> /tmp/p.log 2>&1"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{"Tty":false}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== /app/gateway-server.jar ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))
