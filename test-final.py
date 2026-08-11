"""Use timeout + dd to bound the read"""
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

# Write a python one-liner to /tmp/probe.py inside container, then run it
run_probe("TEST 1: Write python one-liner to container /tmp/probe.py and execute it",
    "cat > /tmp/probe.py << 'PYEOF'\n"
    "import socket, sys\n"
    "s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n"
    "s.settimeout(10)\n"
    "s.connect(('127.0.0.1', 8080))\n"
    "req = b'GET /api/connections/list HTTP/1.1\\r\\nHost: 127.0.0.1\\r\\nConnection: close\\r\\n\\r\\n'\n"
    "s.sendall(req)\n"
    "data = b''\n"
    "while True:\n"
    "    try:\n"
    "        chunk = s.recv(4096)\n"
    "        if not chunk: break\n"
    "        data += chunk\n"
    "    except socket.timeout:\n"
    "        break\n"
    "s.close()\n"
    "sys.stdout.write(data.decode('utf-8', errors='replace'))\n"
    "PYEOF\n"
    "which python3 || which python; python3 /tmp/probe.py 2>&1 || python /tmp/probe.py 2>&1",
    timeout=15)

# Fallback: pure bash with timeout and explicit read
run_probe("TEST 2: Pure bash HTTP probe with timeout 5s",
    "timeout 5 bash -c '"
    "  exec 3<>/dev/tcp/127.0.0.1/8080; "
    "  printf \"GET /api/connections/list HTTP/1.0\\r\\nHost: 127.0.0.1\\r\\n\\r\\n\" >&3; "
    "  timeout 3 dd bs=1 count=4096 <&3 2>/dev/null | head -50; "
    "' 2>&1",
    timeout=10)

run_probe("TEST 3: All listening ports hex/dec",
    "awk '$4==\"0A\" {split($2,a,\":\"); printf \"%s -> %d\\n\", $2, strtonum(\"0x\"a[2])}' /proc/net/tcp /proc/net/tcp6 2>&1 | sort -u",
    timeout=5)

# Check if it's a spring boot fat jar
run_probe("TEST 4: Check jar layout (BOOT-INF? layers?)",
    "unzip -l /app/gateway-server.jar 2>&1 | head -20; "
    "echo '---'; "
    "unzip -l /app/gateway-server.jar 2>&1 | grep -E 'application\\.(yml|properties)' | head -5",
    timeout=10)
