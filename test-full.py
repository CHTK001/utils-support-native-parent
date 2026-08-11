"""完整功能测试 - 真实 HTTP API 调用"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'
CONTAINER = 'gateway-server'

def exec_run(cmd, timeout=10):
    full_cmd = f'{cmd} > /tmp/probe.log 2>&1; echo "DONE"'
    body = json.dumps({
        "Cmd": ["bash", "-c", full_cmd],
        "AttachStdout": False, "AttachStderr": False
    }).encode('utf-8')
    req = urllib.request.Request(f'{URL}/containers/{CONTAINER}/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    r = urllib.request.urlopen(req)
    exec_id = json.loads(r.read())['Id']
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/exec/{exec_id}/start',
        data=b'{"Tty":false,"Detach":false}',
        headers={'Content-Type': 'application/json'},
        method='POST')).read()
    time.sleep(timeout)

def read_file(path):
    req = urllib.request.Request(f'{URL}/containers/{CONTAINER}/archive?path={path}')
    r = urllib.request.urlopen(req, timeout=10)
    tar_data = r.read()
    if len(tar_data) < 512:
        return f"(tar too small: {len(tar_data)} bytes)"
    size_str = tar_data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
    size = int(size_str, 8) if size_str else 0
    return tar_data[512:512+size].decode('utf-8', errors='replace')

def run_probe(label, cmd, timeout=10):
    print(f"\n{'=' * 70}\n{label}\n{'=' * 70}")
    exec_run(cmd, timeout=timeout)
    out = read_file('/tmp/probe.log')
    print(out)

# helper: HTTP GET via bash
def http_get(path):
    return (
        f"timeout 5 bash -c '"
        f"exec 3<>/dev/tcp/127.0.0.1/8080; "
        f"printf \"GET {path} HTTP/1.0\\r\\nHost: 127.0.0.1\\r\\n\\r\\n\" >&3; "
        f"timeout 3 dd bs=1 count=8192 <&3 2>/dev/null"
        f"'"
    )

# 1. /api/connections/list
run_probe("TEST 1: GET /api/connections/list (协议列表)",
    http_get("/api/connections/list"), timeout=10)

# 2. /api/connections/keys (密钥列表)
run_probe("TEST 2: GET /api/connections/keys (key 列表)",
    http_get("/api/connections/keys"), timeout=10)

# 3. /api/connections/authenticate (认证)
run_probe("TEST 3: POST /api/connections/authenticate",
    "timeout 5 bash -c '"
    "exec 3<>/dev/tcp/127.0.0.1/8080; "
    "printf \"POST /api/connections/authenticate HTTP/1.0\\r\\nHost: 127.0.0.1\\r\\nContent-Type: application/json\\r\\nContent-Length: 50\\r\\n\\r\\n\" >&3; "
    "printf \"{\\\"protocol\\\":\\\"SSH\\\",\\\"host\\\":\\\"127.0.0.1\\\",\\\"port\\\":22}\" >&3; "
    "timeout 3 dd bs=1 count=8192 <&3 2>/dev/null"
    "'",
    timeout=10)

# 4. WS handshake
run_probe("TEST 4: WS handshake on 8182",
    "timeout 5 bash -c '"
    "exec 3<>/dev/tcp/127.0.0.1/8182; "
    "printf \"GET / HTTP/1.1\\r\\nHost: 127.0.0.1\\r\\nUpgrade: websocket\\r\\nConnection: Upgrade\\r\\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\\r\\nSec-WebSocket-Version: 13\\r\\n\\r\\n\" >&3; "
    "timeout 3 dd bs=1 count=4096 <&3 2>/dev/null | head -c 500"
    "'",
    timeout=10)
