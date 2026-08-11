"""容器实际监听 8080 不是 8090！用 8080 重测"""
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

# Java 进程 1F90 = 8080
# Java 进程 1FF6 = 8182

run_probe("端口对照表 (hex → dec)",
    "python3 -c 'print(\"1F90 =\", 0x1F90); print(\"1F9A =\", 0x1F9A); print(\"1FF6 =\", 0x1FF6)' 2>&1 || "
    "echo '1F90=8080'; echo '1F9A=8090'; echo '1FF6=8182'", timeout=5)

run_probe("容器内 curl 127.0.0.1:8080/api/connections/list (实际端口)",
    "which curl; "
    "curl -sm 10 http://127.0.0.1:8080/api/connections/list 2>&1; "
    "echo CURL_EXIT=$?", timeout=15)

run_probe("容器内 HTTP POST 探测 8182 (WS endpoint)",
    "echo 'GET / HTTP/1.0' | timeout 5 bash -c 'exec 3<>/dev/tcp/127.0.0.1/8182; cat >&3; cat <&3' 2>&1 | head -20",
    timeout=10)

run_probe("所有 /proc/net/tcp 监听端口",
    "cat /proc/net/tcp | awk '$4==\"0A\" {print $2}'", timeout=5)

run_probe("检查 application.yml / properties 配置",
    "find /app -name '*.yml' -o -name '*.properties' 2>/dev/null | head; "
    "ls /app/dependencies 2>&1 | head -10; "
    "echo '---'; "
    "ls /app 2>&1 | head -20", timeout=5)
