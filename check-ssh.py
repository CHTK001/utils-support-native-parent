"""
Read container logs via tar archive properly
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'

def run_in_container(cmd, timeout=8):
    full_cmd = f'{cmd} > /tmp/p.log 2>&1; echo done > /tmp/done'
    body = json.dumps({"Cmd": ["bash", "-c", full_cmd]}).encode('utf-8')
    req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    r = urllib.request.urlopen(req)
    eid = json.loads(r.read())['Id']
    urllib.request.urlopen(urllib.request.Request(
        f'{URL}/exec/{eid}/start', data=b'{}',
        headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)
    r = urllib.request.urlopen(f'{URL}/exec/{eid}/json')
    return json.loads(r.read())['ExitCode']

def read_file(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=10)
    data = r.read()
    # Tar header is 512 bytes, then data, then padded
    if len(data) < 512:
        return f"(tar too small: {len(data)})"
    size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
    size = int(size_str, 8) if size_str else 0
    return data[512:512+size].decode('utf-8', errors='replace')

# Check if SSH server runs in container
ec = run_in_container("""echo === netstat ===; netstat -tln 2>/dev/null | grep 22 || ss -tln 2>/dev/null | grep 22; echo === /etc/passwd users ===; cat /etc/passwd | head -10; echo === sshd ===; which sshd; echo done""")
print("=" * 60)
print("Container SSH/netstat:")
print(read_file('/tmp/p.log'))

# Check sandbox's actual SSH (tester)
ec = run_in_container("""echo === local SSH port ===; (echo > /dev/tcp/127.0.0.1/22) 2>&1 && echo OPEN || echo CLOSED; echo === local SSH port gateway ===; (echo > /dev/tcp/172.18.0.1/22) 2>&1 && echo GW_OPEN || echo GW_CLOSED; echo done""", timeout=5)
print("=" * 60)
print("Local SSH from container:")
print(read_file('/tmp/p.log'))

# Test: try authentication from gateway itself
print("=" * 60)
print("Test: check if java has any test creds configured")
# Look at the surefire tests we wrote
ec = run_in_container("""echo === user tester ===; id tester 2>&1; echo === tester home ===; ls -la /home/tester 2>&1 | head -5; echo done""", timeout=5)
print(read_file('/tmp/p.log'))
