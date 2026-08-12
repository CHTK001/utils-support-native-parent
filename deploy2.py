"""
Simpler: keep gateway-server container, just upload new jar and restart it
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
        return f"HTTP {e.code}"

# 1. Check if gateway-server exists
existed = json.loads(req('/containers/gateway-server/json')) if 'HTTP' not in (s := req('/containers/gateway-server/json')) else None
# Simpler: just try to stop
print("=== 1. Stop gateway-server ===")
try:
    req('/containers/gateway-server/stop?t=5', 'POST', {})
    print("OK stopped")
except: pass
time.sleep(5)

# 2. Upload new jar (container is stopped, but filesystem still accessible)
print("\n=== 2. Upload new gateway-server.jar ===")
with open(r"D:\ch\project\gateway-server.jar", "rb") as f:
    jar_bytes = f.read()
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode='w') as tar:
    info = tarfile.TarInfo(name='gateway-server.jar')
    info.size = len(jar_bytes)
    tar.addfile(info, io.BytesIO(jar_bytes))
tar_bytes = buf.getvalue()

r = urllib.request.Request(f'{URL}/containers/gateway-server/archive?path=/app',
    data=tar_bytes, method='PUT')
r.add_header('Content-Type', 'application/x-tar')
try:
    urllib.request.urlopen(r, timeout=120)
    print(f"Uploaded {len(jar_bytes)} bytes")
except Exception as e:
    print(f"Upload err: {e}")
    # Maybe container is gone - need to create
    print("Container gone, will recreate after upload")

# 3. Start container
print("\n=== 3. Start gateway-server ===")
try:
    req('/containers/gateway-server/start', 'POST', {})
    print("Started")
except Exception as e:
    print(f"Start err: {e}")
    # Need to create
    print("Trying to create...")
    cfg = {
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
    result = req('/containers/create?name=gateway-server', 'POST', cfg)
    print(f"create: {result[:200]}")
    try:
        new_id = json.loads(result)['Id']
        req(f'/containers/{new_id}/start', 'POST', {})
        print("Created + started")
    except: pass

# 4. Wait for boot
print("\n=== 4. Wait 25s for gateway boot + guacd auto-bootstrap ===")
time.sleep(25)

state = json.loads(req('/containers/gateway-server/json'))
print(f"Gateway state: {state['State']['Status']} Running={state['State']['Running']}")

try:
    guacd_state = json.loads(req('/containers/guacd/json'))
    print(f"🎉 Guacd container auto-created! State: {guacd_state['State']['Status']}")
    print(f"   Image: {guacd_state['Image']}")
    print(f"   Ports: {guacd_state['NetworkSettings']['Ports']}")
except Exception as e:
    print(f"guacd container NOT found: {e}")
