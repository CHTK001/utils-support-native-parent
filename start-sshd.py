"""
Start sshd in container, set root password
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
        f'{URL}/exec/{eid}/start', data=b'{"Tty":false}',
        headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)

def read(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=10)
    data = r.read()
    sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
    return data[512:512+sz].decode('utf-8', errors='replace')

# Generate host keys
run("ssh-keygen -A > /tmp/p.log 2>&1; ls /etc/ssh/ >> /tmp/p.log 2>&1")
print("=== host keys ===")
print(read('/tmp/p.log'))

# Set root password
run("echo 'root:rootpass123' | chpasswd > /tmp/p.log 2>&1; cat /etc/shadow | grep root >> /tmp/p.log 2>&1")
print("=== root password ===")
print(read('/tmp/p.log'))

# Allow root login
run("sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config; grep -E 'PermitRoot|PasswordAuth|PubkeyAuth|UsePAM' /etc/ssh/sshd_config > /tmp/p.log 2>&1")
print("=== sshd_config ===")
print(read('/tmp/p.log'))

# Start sshd
run("mkdir -p /var/run/sshd; /usr/sbin/sshd -D &> /tmp/sshd.log & sleep 2; ss -tln | grep 22 >> /tmp/p.log 2>&1; echo done")
print("=== sshd started ===")
print(read('/tmp/p.log'))

# Test SSH from inside container
import socket
s = socket.socket()
s.settimeout(3)
try:
    s.connect(('127.0.0.1', 22))
    print("=== TCP 127.0.0.1:22 OPEN from inside container! ===")
    s.close()
except Exception as e:
    print(f"=== TCP from inside: {e} ===")
