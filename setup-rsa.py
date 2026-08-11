import urllib.request, json, time, base64
URL = 'http://172.16.0.40:2375'

# Read pub and key
with open(r'D:\ch\project\test_id_rsa.pub', 'r') as f:
    pub = f.read().strip()

# Add pub to authorized_keys
body = json.dumps({"Cmd": ["sh", "-c", f"echo '{pub}' > /root/.ssh/authorized_keys; chmod 600 /root/.ssh/authorized_keys; cat /root/.ssh/authorized_keys > /tmp/p.log 2>&1"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(2)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== authorized_keys ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))

# Restart sshd to pick up new keys
body = json.dumps({"Cmd": ["sh", "-c", "kill $(pgrep sshd) 2>&1; sleep 1; /usr/sbin/sshd > /tmp/sshd.log 2>&1; sleep 1; pgrep sshd > /tmp/p.log 2>&1"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== sshd restarted ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))
