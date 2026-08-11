"""
真实测试 gateway-server (容器内)
sandbox 不能访问 host:18090，但可以从容器内访问 127.0.0.1:8090 (同 namespace)
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'

def exec_in_container(cmd):
    """exec command in gateway-server container, return (exitcode, output)"""
    body = json.dumps({
        "Cmd": ["bash", "-c", cmd],
        "AttachStdout": True, "AttachStderr": True
    }).encode('utf-8')
    req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    r = urllib.request.urlopen(req)
    exec_id = json.loads(r.read())['Id']
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/exec/{exec_id}/start',
        data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(3)
    r4 = urllib.request.urlopen(f'{URL}/exec/{exec_id}/json')
    d4 = json.loads(r4.read())
    return d4['ExitCode'], d4.get('Output')

print("=" * 70)
print("TEST 1: container 进程检查 (ProcessStatus + Java thread dump)")
print("=" * 70)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/json')
d = json.loads(r.read())
print(f"Status: {d['State']['Status']}")
print(f"Running: {d['State']['Running']}")
print(f"Pid: {d['State']['Pid']}")
print(f"ExitCode: {d['State']['ExitCode']}")
print(f"StartedAt: {d['State']['StartedAt']}")
print(f"Ports: {d['NetworkSettings']['Ports']}")

print("\n" + "=" * 70)
print("TEST 2: 容器内监听端口检查 (ss/netstat)")
print("=" * 70)
ec, out = exec_in_container(
    "(ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null) | grep -E '8080|8090|8182|9001'"
)
print(f"ExitCode: {ec}")
if out:
    decoded = out.decode('utf-8', errors='replace') if isinstance(out, bytes) else out
    print(decoded)
else:
    print("(no output captured - Docker API 1.41 quirk)")
    # Try /proc instead
    ec2, out2 = exec_in_container(
        "for pid in $(ls /proc 2>/dev/null | grep -E '^[0-9]+$'); do "
        "  if [ -d /proc/$pid/net ]; then "
        "    ls -la /proc/$pid/exe 2>/dev/null | head -1; "
        "  fi; "
        "done | head -20"
    )
    print(f"Procs: {out2}")

print("\n" + "=" * 70)
print("TEST 3: 容器内 curl 127.0.0.1:8090/api/connections/list")
print("=" * 70)
# Check if curl exists
ec, out = exec_in_container("which curl wget")
print(f"curl/wget check: {out}")
# Use wget instead since it's usually present
ec, out = exec_in_container(
    "wget -q -O - --timeout=10 http://127.0.0.1:8090/api/connections/list 2>&1; "
    "echo EXIT=$?"
)
print(f"ExitCode: {ec}")
if out:
    decoded = out.decode('utf-8', errors='replace') if isinstance(out, bytes) else out
    print(f"Response body:\n{decoded}")

print("\n" + "=" * 70)
print("TEST 4: 容器内 curl 127.0.0.1:8090 全路由列表 (Spring Boot actuator 风格)")
print("=" * 70)
ec, out = exec_in_container(
    "for path in / /api/connections/list /api/connections/keys "
    "/api/connections/authenticate /api/connections/disconnect "
    "/actuator/health /api/health; do "
    "  echo \"=== GET $path ===\"; "
    "  wget -q -O - --timeout=5 http://127.0.0.1:8090$path 2>&1 | head -5; "
    "  echo; "
    "done"
)
print(f"ExitCode: {ec}")
if out:
    decoded = out.decode('utf-8', errors='replace') if isinstance(out, bytes) else out
    print(decoded)
