"""
Direct sh exec
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, timeout=5):
    body = json.dumps({"Cmd": ["sh", "-c", cmd]}).encode('utf-8')
    req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    r = urllib.request.urlopen(req)
    eid = json.loads(r.read())['Id']
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/exec/{eid}/start', data=b'{}',
        headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)

def read(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=10)
    data = r.read()
    if len(data) < 512: return ""
    size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
    size = int(size_str, 8) if size_str else 0
    return data[512:512+size].decode('utf-8', errors='replace')

# Test 1: whoami
run("whoami > /tmp/p.log 2>&1; id >> /tmp/p.log 2>&1")
print("=== whoami/id ===")
print(read("/tmp/p.log"))

# Test 2: cat /etc/passwd
run("cat /etc/passwd > /tmp/p.log 2>&1")
print("\n=== /etc/passwd ===")
print(read("/tmp/p.log"))

# Test 3: try SSH
run("ls /tmp/test-ssh 2>&1 > /tmp/p.log; echo OK")
print("\n=== /tmp/p.log (ls /tmp/test-ssh) ===")
print(read("/tmp/p.log"))

# Try to write a tester password via the gateway API
# Use existing surefire test
print("\n=== Test 1: try writing to /tmp ===")
run("touch /tmp/test-write; echo $? > /tmp/p.log")
print(read("/tmp/p.log"))

# Test: check what JSCH test would do
# Look at our RealSshInteractiveTest that worked earlier
# It uses tester/TestPass!123@127.0.0.1:22
# But that's the sandbox's local SSH, not the container's

# Test: from inside container, can we connect to 127.0.0.1:22?
run("echo TEST > /tmp/p.log")
print("\n=== base test ===")
print(read("/tmp/p.log"))
