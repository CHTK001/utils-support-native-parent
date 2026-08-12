"""
Deploy v6 with compile-from-source support
"""
import urllib.request, json, time, sys, io, tarfile
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def req(path, method='GET', data=None):
    body = json.dumps(data).encode() if data else None
    r = urllib.request.Request(f'{URL}{path}', data=body,
        headers={'Content-Type': 'application/json'} if data else {},
        method=method)
    try:
        return urllib.request.urlopen(r, timeout=30).read().decode()
    except urllib.error.HTTPError as e:
        return f"HTTP {e.code}: {e.reason}"

# Build new image
print("=== Build gateway-server:v6 ===")
with open(r"D:\ch\project\gateway-server.jar", "rb") as f:
    jar_bytes = f.read()
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode='w') as tar:
    dockerfile = b'FROM gateway-server:latest\nCOPY gateway-server.jar /app/gateway-server.jar\n'
    info = tarfile.TarInfo(name='Dockerfile')
    info.size = len(dockerfile)
    tar.addfile(info, io.BytesIO(dockerfile))
    info = tarfile.TarInfo(name='gateway-server.jar')
    info.size = len(jar_bytes)
    tar.addfile(info, io.BytesIO(jar_bytes))
build_tar = buf.getvalue()

import socket
socket.setdefaulttimeout(600)
r = urllib.request.Request(f'{URL}/build?t=gateway-server:v6&dockerfile=Dockerfile&rm=true&forcerm=true',
    data=build_tar, method='POST')
r.add_header('Content-Type', 'application/x-tar')
try:
    resp = urllib.request.urlopen(r, timeout=600)
    print(f"Build OK")
except Exception as e:
    print(f"Build err: {e}")

# Stop and remove
print("\n=== Stop and remove gateway-server ===")
try: req('/containers/gateway-server/stop?t=5', 'POST', {})
except: pass
time.sleep(5)
try: req('/containers/gateway-server?force=true&v=true', 'DELETE')
except: pass

# Create new
print("\n=== Create new container ===")
cfg = {
    "Image": "gateway-server:v6",
    "ExposedPorts": {"8080/tcp": {}, "8182/tcp": {}},
    "HostConfig": {
        "PortBindings": {
            "8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18090"}],
            "8182/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18182"}],
        },
        "RestartPolicy": {"Name": "always"},
        "SecurityOpt": ["seccomp=unconfined"],
        "Env": [
            "GATEWAY_CP=/app/gateway-server.jar:/app/dependencies/*",
            "JAVA_OPTS=-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8",
        ],
    },
    "Cmd": [
        "/bin/sh", "-c",
        "exec /opt/java/openjdk/bin/java $JAVA_OPTS -cp $GATEWAY_CP com.chua.gateway.server.GatewayServerApplication"
    ]
}
result = req('/containers/create?name=gateway-server', 'POST', cfg)
new_id = json.loads(result)['Id']
req(f'/containers/{new_id}/start', 'POST', {})
print("Started")

# Wait for compile + boot (10+ min)
print("\n=== Wait 30s for initial boot + download ===")
time.sleep(30)
state = json.loads(req('/containers/gateway-server/json'))
print(f"Gateway: {state['State']['Status']} Running={state['State']['Running']}")

# Check cache
print("\n=== Cache after 30s ===")
body = json.dumps({"Cmd": ["sh", "-c", "find /root/.utils-support-gateway 2>/dev/null > /tmp/p.log"]}).encode()
r = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
eid = json.loads(urllib.request.urlopen(r).read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
    data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print(data[512:512+sz].decode('utf-8', errors='replace'))

# Check compile status
print("\n=== Process check (guacd sub-process?) ===")
body = json.dumps({"Cmd": ["sh", "-c", "ps aux | grep -E 'guacd|configure|make' | grep -v grep > /tmp/p.log 2>&1; echo done"]}).encode()
r = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
eid = json.loads(urllib.request.urlopen(r).read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
    data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print(data[512:512+sz].decode('utf-8', errors='replace'))
