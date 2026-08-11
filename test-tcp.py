"""用 bash /dev/tcp 真实 HTTP 探测"""
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

# === Test HTTP on 8080 (the actual port) ===
run_probe("TEST A: bash /dev/tcp HTTP GET 127.0.0.1:8080/api/connections/list",
    "exec 3<>/dev/tcp/127.0.0.1/8080 && "
    "  printf 'GET /api/connections/list HTTP/1.1\\r\\nHost: 127.0.0.1\\r\\nConnection: close\\r\\n\\r\\n' >&3; "
    "  cat <&3; "
    "echo 'EXIT='$?",
    timeout=10)

run_probe("TEST B: bash /dev/tcp HTTP GET 127.0.0.1:8080/",
    "exec 3<>/dev/tcp/127.0.0.1/8080 && "
    "  printf 'GET / HTTP/1.1\\r\\nHost: 127.0.0.1\\r\\nConnection: close\\r\\n\\r\\n' >&3; "
    "  cat <&3; "
    "echo 'EXIT='$?",
    timeout=10)

# === Test WS handshake on 8182 ===
run_probe("TEST C: WS handshake on 8182",
    "exec 3<>/dev/tcp/127.0.0.1/8182 && "
    "  printf 'GET / HTTP/1.1\\r\\nHost: 127.0.0.1\\r\\nUpgrade: websocket\\r\\nConnection: Upgrade\\r\\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\\r\\nSec-WebSocket-Version: 13\\r\\n\\r\\n' >&3; "
    "  cat <&3; "
    "echo 'EXIT='$?",
    timeout=10)

run_probe("TEST D: 查 application.yml/properties",
    "ls -la /app/dependencies 2>&1 | head -20; "
    "echo '---'; "
    "unzip -p /app/gateway-server.jar BOOT-INF/classes/application.yml 2>/dev/null || "
    "  unzip -p /app/gateway-server.jar BOOT-INF/classes/application.properties 2>/dev/null || "
    "  echo NO_YML",
    timeout=10)

run_probe("TEST E: 容器内 Java 进程监听端口 (lsof on PID 1)",
    "ls /proc/1/fd 2>&1 | wc -l; "
    "cat /proc/1/net/tcp 2>&1 | awk '$4==\"0A\" {print $2}' | head; "
    "echo '---'; "
    "cat /proc/1/cmdline 2>&1 | tr '\\0' ' '; echo",
    timeout=5)
