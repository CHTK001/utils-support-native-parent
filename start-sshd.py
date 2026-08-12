"""
Start sshd in container + create test user + retest SSH
"""
import urllib.request, json, sys, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, wait=10):
    body = json.dumps({'Cmd':['sh','-c',cmd]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type':'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false,"Detach":true}', headers={'Content-Type':'application/json'}, method='POST')).read()
    for i in range(wait*10):
        time.sleep(0.1)
        ins = json.loads(urllib.request.urlopen(f'{URL}/exec/{eid}/json').read())
        if not ins.get('Running'): break
    return ins

def get(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=30)
    data = r.read()
    if len(data) < 512: return ''
    size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8', errors='replace')
    try: size = int(size_str, 8)
    except: size = 0
    return data[512:512+size].decode('utf-8', errors='replace')

# Check if sshd installed
print('=== which sshd ===')
run('which sshd 2>&1 > /tmp/s.log')
print(get('/tmp/s.log'))

# Install if not
print('=== install openssh-server ===')
run('apt-get install -y --no-install-recommends openssh-server 2>&1 | tail -3 > /tmp/s.log', wait=120)
print(get('/tmp/s.log'))

# Setup sshd
print('\n=== setup sshd ===')
run('mkdir -p /var/run/sshd && echo "root:rootpass123" | chpasswd 2>&1 > /tmp/s.log')
print(get('/tmp/s.log'))

# Configure PermitRootLogin
run("sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config; sed -i 's/^#PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config; grep -E 'PermitRootLogin|PasswordAuthentication' /etc/ssh/sshd_config > /tmp/s.log")
print(get('/tmp/s.log'))

# Start sshd
print('=== start sshd ===')
run('/usr/sbin/sshd -D > /tmp/sshd.log 2>&1 & sleep 2; ps -ef | grep sshd | grep -v grep > /tmp/s.log')
print(get('/tmp/s.log'))

# Check 22 listening
print('\n=== port 22 ===')
run('ss -tln | grep 22 > /tmp/s.log')
print(get('/tmp/s.log'))

# Now retry SSH authenticate
print('=== POST /api/connections/authenticate (SSH) ===')
body = '{"mode":"custom","protocol":"SSH","host":"127.0.0.1","port":22,"user":"root","password":"rootpass123"}'
run(f"curl -sS -m 10 -X POST -H 'Content-Type: application/json' -d '{body}' http://127.0.0.1:8080/api/connections/authenticate > /tmp/s.log")
print(get('/tmp/s.log'))

# Get tunnel id
print('\n=== check active tunnels ===')
run('curl -sS -m 5 http://127.0.0.1:8080/api/connections/keys > /tmp/s.log')
print(get('/tmp/s.log'))