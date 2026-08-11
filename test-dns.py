"""
Test DNS resolution from container
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

# Test: Can container resolve 172.16.0.40?
run("nslookup 172.16.0.40 2>&1 > /tmp/p.log; cat /etc/resolv.conf >> /tmp/p.log 2>&1; getent hosts 172.16.0.40 >> /tmp/p.log 2>&1; echo done >> /tmp/p.log")
print("=== DNS test ===")
print(read("/tmp/p.log"))

# Test: Can container reach 172.16.0.40:22 with raw TCP?
print("\n=== TCP test to 172.16.0.40:22 ===")
run("timeout 3 sh -c '(echo > /dev/tcp/172.16.0.40/22) && echo OPEN || echo CLOSED' >> /tmp/p.log 2>&1")
print(read("/tmp/p.log"))

# Test: to host.docker.internal?
print("\n=== host.docker.internal test ===")
run("timeout 3 sh -c '(echo > /dev/tcp/host.docker.internal/22) && echo OPEN || echo CLOSED' >> /tmp/p.log 2>&1")
print(read("/tmp/p.log"))

# Test: container IP + Docker network
print("\n=== Container network ===")
run("hostname -i >> /tmp/p.log 2>&1; cat /etc/hosts >> /tmp/p.log 2>&1; ip route >> /tmp/p.log 2>&1; echo done")
print(read("/tmp/p.log"))

# Test: gateway IP (what's the docker network?)
run("cat /etc/resolv.conf > /tmp/p.log 2>&1; ip route >> /tmp/p.log 2>&1; ip addr >> /tmp/p.log 2>&1; echo done")
print("\n=== Network config ===")
print(read("/tmp/p.log")[:2000])
