"""
Recreate gateway-server container from scratch with new jar
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

# 1. Upload new jar to a temp location via image build context
# Actually - simpler: just upload jar to /tmp on host first, then create container with bind mount
# But we don't have host access. Use the gateway-server image as base.

# 2. Create a new image from jar via docker exec
# Simpler: build a new image with the new jar using /build API
print("=== Build new image gateway-server:v4 with new jar ===")
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
r = urllib.request.Request(f'{URL}/build?t=gateway-server:v4&dockerfile=Dockerfile&rm=true&forcerm=true',
    data=build_tar, method='POST')
r.add_header('Content-Type', 'application/x-tar')
try:
    resp = urllib.request.urlopen(r, timeout=600)
    print(f"Build OK: last 200: {resp.read().decode()[-200:]}")
except Exception as e:
    print(f"Build err: {e}")
    if hasattr(e, 'read'):
        body = e.read().decode('utf-8', errors='replace')
        print(f"Body: {body[:500]}")

# 3. Create container from v4
print("\n=== Create container ===")
cfg = {
    "Image": "gateway-server:v4",
    "ExposedPorts": {"8080/tcp": {}, "8182/tcp": {}},
    "HostConfig": {
        "PortBindings": {
            "8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18090"}],
            "8182/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18182"}],
        },
        "RestartPolicy": {"Name": "always"},
        "SecurityOpt": ["seccomp=unconfined"],
        "ExtraHosts": ["guacd:172.18.0.1"],
        "Env": [
            "GATEWAY_CP=/app/gateway-server.jar:/app/dependencies/*",
            "JAVA_OPTS=-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8",
            "GATEWAY_GUACD_HOST=guacd",
            "GATEWAY_GUACD_PORT=4822",
            "DOCKER_API_URL=http://172.18.0.1:2375",
        ],
    },
    "Cmd": [
        "/bin/sh", "-c",
        "exec /opt/java/openjdk/bin/java $JAVA_OPTS -cp $GATEWAY_CP -Dguacd.host=$GATEWAY_GUACD_HOST -Dguacd.port=$GATEWAY_GUACD_PORT -Ddocker.api.url=$DOCKER_API_URL com.chua.gateway.server.GatewayServerApplication"
    ]
}
result = req('/containers/create?name=gateway-server', 'POST', cfg)
print(f"create: {result[:200]}")
try:
    new_id = json.loads(result)['Id']
    req(f'/containers/{new_id}/start', 'POST', {})
    print("Started")
except Exception as e:
    print(f"Create/start err: {e}")

# 4. Wait
print("\n=== Wait 25s ===")
time.sleep(25)

state = json.loads(req('/containers/gateway-server/json'))
print(f"Gateway: {state['State']['Status']} Running={state['State']['Running']}")

try:
    g = json.loads(req('/containers/guacd/json'))
    print(f"🎉 Guacd auto-created! State: {g['State']['Status']}")
    print(f"   Image: {g['Image']}")
    print(f"   Ports: {g['NetworkSettings']['Ports']}")
except Exception as e:
    print(f"guacd not found: {e}")

# 5. Test API
print("\n=== Test API ===")
try:
    s = json.loads(req('/containers/gateway-server/json'))
    ports = s['NetworkSettings']['Ports']
    print(f"Gateway ports: {ports}")
except: pass

# Test gateway HTTP
import urllib.error
try:
    r = urllib.request.urlopen('http://172.16.0.40:18090/api/connections/list', timeout=10)
    print(f"Gateway HTTP: {r.status} {r.read().decode()[:200]}")
except Exception as e:
    print(f"Gateway HTTP err: {e}")
