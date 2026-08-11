"""
Fix: stop + remove + recreate with correct port mapping 18090:8080
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'
CONTAINER = 'gateway-server'
IMAGE = 'gateway-server:latest'

def req(path, method='GET', data=None):
    body = json.dumps(data).encode('utf-8') if data else None
    r = urllib.request.Request(f'{URL}{path}', data=body,
        headers={'Content-Type': 'application/json'} if data else {},
        method=method)
    return urllib.request.urlopen(r, timeout=30).read().decode('utf-8', errors='replace')

# 1. Stop and remove
print("=== 1. Stop container ===")
try:
    req(f'/containers/{CONTAINER}/stop', 'POST', {"t": 5})
    print("OK stopped")
except Exception as e:
    print(f"stop: {e}")

print("\n=== 2. Remove container ===")
try:
    req(f'/containers/{CONTAINER}?force=true&v=true', 'DELETE')
    print("OK removed")
except Exception as e:
    print(f"rm: {e}")

# 2. Recreate with correct port mapping 18090:8080
print("\n=== 3. Create new container (port 18090:8080, 18182:8182) ===")
new_cfg = {
    "Image": IMAGE,
    "ExposedPorts": {"8080/tcp": {}, "8182/tcp": {}},
    "HostConfig": {
        "PortBindings": {
            "8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18090"}],
            "8182/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18182"}],
        },
        "RestartPolicy": {"Name": "always"},
        "SecurityOpt": ["seccomp=unconfined"],
        "ExtraHosts": ["guacd:172.18.0.1"],  # fallback if DNS fails
        "Env": [
            "GATEWAY_CP=/app/gateway-server.jar:/app/dependencies/*",
            "JAVA_OPTS=-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8",
            "GATEWAY_GUACD_HOST=guacd",
            "GATEWAY_GUACD_PORT=4822",
        ],
    },
    "Cmd": [
        "/bin/sh", "-c",
        "exec /opt/java/openjdk/bin/java $JAVA_OPTS -cp $GATEWAY_CP -Dguacd.host=$GATEWAY_GUACD_HOST -Dguacd.port=$GATEWAY_GUACD_PORT com.chua.gateway.server.GatewayServerApplication"
    ]
}
result = req('/containers/create?name=' + CONTAINER, 'POST', new_cfg)
print(f"create: {result}")
new_id = json.loads(result).get('Id')
print(f"new container id: {new_id}")

print("\n=== 4. Start new container ===")
start = req(f'/containers/{new_id}/start', 'POST', {})
print(f"start: {start}")

# Wait for boot
print("\n=== 5. Wait 10s for boot ===")
time.sleep(10)

# Verify
print("\n=== 6. Verify new container state ===")
state = json.loads(req(f'/containers/{new_id}/json', 'GET'))
print(f"State: {state['State']['Status']} Running={state['State']['Running']} Pid={state['State']['Pid']}")
print(f"Ports: {state['NetworkSettings']['Ports']}")

print("\n=== 7. tail logs ===")
# get logs
r = urllib.request.urlopen(f'{URL}/containers/{new_id}/logs?stdout=true&stderr=true&tail=15')
logs = r.read().decode('utf-8', errors='replace')
print(logs[-1500:])
