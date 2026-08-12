"""
1. Stop and remove existing guacd container (test bootstrap auto-start)
2. Deploy new gateway-server.jar
3. Verify gateway auto-creates + starts guacd container
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def req(path, method='GET', data=None):
    body = json.dumps(data).encode() if data else None
    r = urllib.request.Request(f'{URL}{path}', data=body,
        headers={'Content-Type': 'application/json'} if data else {},
        method=method)
    return urllib.request.urlopen(r, timeout=30).read().decode()

# 1. Stop and remove guacd container
print("=== 1. Stop and remove existing guacd container ===")
try:
    req('/containers/guacd/stop?t=5', 'POST', {})
    print("OK stopped")
except: pass
time.sleep(3)
try:
    req('/containers/guacd?force=true&v=true', 'DELETE')
    print("OK removed")
except Exception as e:
    print(f"remove err: {e}")

# 2. Verify gone
time.sleep(2)
try:
    state = json.loads(req('/containers/guacd/json'))
    print(f"guacd still exists: state={state.get('State')}")
except Exception as e:
    print(f"guacd gone: {e}")

# 3. Deploy new gateway-server.jar
print("\n=== 2. Deploy new gateway-server.jar ===")
import io, tarfile
with open(r"D:\ch\project\gateway-server.jar", "rb") as f:
    jar_bytes = f.read()
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode='w') as tar:
    info = tarfile.TarInfo(name='gateway-server.jar')
    info.size = len(jar_bytes)
    tar.addfile(info, io.BytesIO(jar_bytes))
tar_bytes = buf.getvalue()

# Upload
r = urllib.request.Request(f'{URL}/containers/gateway-server/archive?path=/app',
    data=tar_bytes, method='PUT')
r.add_header('Content-Type', 'application/x-tar')
urllib.request.urlopen(r, timeout=120)
print(f"Uploaded {len(jar_bytes)} bytes")

# 4. Stop gateway container
print("\n=== 3. Stop gateway-server container ===")
try:
    req('/containers/gateway-server/stop?t=5', 'POST', {})
    print("OK stopped")
except: pass
time.sleep(5)

# 5. Remove
try:
    req('/containers/gateway-server?force=true&v=true', 'DELETE')
    print("OK removed")
except: pass
time.sleep(2)

# 6. Recreate with new image (image id stays same, but new jar)
print("\n=== 4. Recreate gateway-server container ===")
new_cfg = {
    "Image": "gateway-server:latest",
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
result = req('/containers/create?name=gateway-server', 'POST', new_cfg)
print(f"create: {result}")
new_id = json.loads(result).get('Id')

req(f'/containers/{new_id}/start', 'POST', {})
print("Started")

# 7. Wait for boot
print("\n=== 5. Wait 20s for boot + guacd auto-bootstrap ===")
time.sleep(20)

# 8. Check container state
state = json.loads(req('/containers/gateway-server/json'))
print(f"Gateway state: {state['State']['Status']} Running={state['State']['Running']}")

# 9. Check guacd container
try:
    guacd_state = json.loads(req('/containers/guacd/json'))
    print(f"🎉 Guacd container auto-created! State: {guacd_state['State']['Status']}")
    print(f"   Image: {guacd_state['Image']}")
    print(f"   Ports: {guacd_state['NetworkSettings']['Ports']}")
except Exception as e:
    print(f"guacd container NOT found: {e}")
