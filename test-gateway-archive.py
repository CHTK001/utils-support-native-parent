"""
Strategy: write output to /tmp/probe.log inside container via exec,
then read it back via /containers/{id}/archive (no streaming serialization issue)
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'
CONTAINER = 'gateway-server'

def exec_run(cmd, timeout=10):
    """exec and write to /tmp/probe.log"""
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
    """read file from container via /containers/{id}/archive"""
    # Get tar
    req = urllib.request.Request(
        f'{URL}/containers/{CONTAINER}/archive?path={path}')
    try:
        r = urllib.request.urlopen(req, timeout=10)
        tar_data = r.read()
        # parse tar manually (simple, since we expect a single file)
        # tar header is 512 bytes, then data, then padded to 512 boundary
        if len(tar_data) < 512:
            return f"(tar too small: {len(tar_data)} bytes)"
        name = tar_data[:100].rstrip(b'\x00').decode('utf-8', errors='replace')
        size_str = tar_data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
        size = int(size_str, 8) if size_str else 0
        data = tar_data[512:512+size]
        return data.decode('utf-8', errors='replace')
    except Exception as e:
        return f"(archive error: {e})"

def run_probe(label, cmd, timeout=10):
    print(f"\n{'=' * 70}\n{label}\n{'=' * 70}")
    exec_run(cmd, timeout=timeout)
    out = read_file('/tmp/probe.log')
    print(out)

# === START ===
run_probe("TEST 1: 容器内端口监听 (cat /proc/net/tcp)",
    "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | head -30", timeout=5)

run_probe("TEST 2: 容器内 wget 127.0.0.1:8090/api/connections/list",
    "wget -O - --timeout=10 http://127.0.0.1:8090/api/connections/list 2>&1", timeout=15)

run_probe("TEST 3: 容器内 netcat 探测 8090 端口 (TCP raw connect)",
    "echo 'GET /api/connections/list HTTP/1.0' | timeout 5 bash -c '"
    "exec 3<>/dev/tcp/127.0.0.1/8090; cat >&3; cat <&3' 2>&1 | head -20", timeout=10)

run_probe("TEST 4: 进程列表 (ps)",
    "ps aux 2>&1 | head -20", timeout=5)

run_probe("TEST 5: Java 进程工作目录 + cmdline",
    "readlink /proc/$(pgrep -f 'java.*Gateway' | head -1)/cwd 2>&1; "
    "cat /proc/$(pgrep -f 'java.*Gateway' | head -1)/cmdline 2>&1 | tr '\\0' ' '; echo", timeout=5)

run_probe("TEST 6: 容器内 jstack/jcmd 简要线程状态",
    "which jstack jcmd 2>&1; ls /opt/java/openjdk/bin/ 2>&1 | head", timeout=5)
