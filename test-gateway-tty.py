"""
With Tty=true, Output is base64-encoded and not null
"""
import urllib.request, json, time, sys, base64
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'

def exec_in_container(cmd, timeout=30):
    body = json.dumps({
        "Cmd": ["bash", "-c", cmd],
        "AttachStdout": True, "AttachStderr": True,
        "Tty": True
    }).encode('utf-8')
    req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    r = urllib.request.urlopen(req)
    exec_id = json.loads(r.read())['Id']
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/exec/{exec_id}/start',
        data=b'{"Tty":true}', headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)
    r4 = urllib.request.urlopen(f'{URL}/exec/{exec_id}/json')
    d4 = json.loads(r4.read())
    out = d4.get('Output')
    if out:
        try:
            return d4['ExitCode'], base64.b64decode(out).decode('utf-8', errors='replace')
        except Exception:
            return d4['ExitCode'], out
    return d4['ExitCode'], None

print("=" * 70)
print("TEST 1: 容器内端口监听 (ss -tlnp)")
print("=" * 70)
ec, out = exec_in_container("ss -tlnp 2>&1 || netstat -tlnp 2>&1", timeout=5)
print(f"ExitCode: {ec}")
print(out)

print("\n" + "=" * 70)
print("TEST 2: 容器内 curl/wget 127.0.0.1:8090 (HTTP API)")
print("=" * 70)
ec, out = exec_in_container(
    "which curl wget; "
    "echo '=== GET /api/connections/list ==='; "
    "(wget -q -O - --timeout=10 http://127.0.0.1:8090/api/connections/list 2>&1 || "
    " curl -sm 10 http://127.0.0.1:8090/api/connections/list 2>&1); "
    "echo; "
    "echo '=== HEAD only ==='; "
    "wget -q --spider --timeout=10 http://127.0.0.1:8090/api/connections/list 2>&1; "
    "echo WGET_EXIT=$?",
    timeout=20
)
print(f"ExitCode: {ec}")
print(out)

print("\n" + "=" * 70)
print("TEST 3: 实际 HTTP 探测 (POST 任何路径)")
print("=" * 70)
ec, out = exec_in_container(
    "wget -q -O - --timeout=15 http://127.0.0.1:8080/api/connections/list 2>&1; "
    "echo WGET_EXIT=$?; "
    "echo '---'; "
    "wget -q -O - --timeout=15 http://127.0.0.1:8090/api/connections/list 2>&1; "
    "echo WGET_EXIT=$?",
    timeout=25
)
print(f"ExitCode: {ec}")
print(out)
