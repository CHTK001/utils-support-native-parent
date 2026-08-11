"""
Test SSH with password - set up container to accept root/root
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

# Reset sshd config to allow password auth for root
body = json.dumps({"Cmd": ["sh", "-c", "sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config; sed -i 's/^#PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config; sed -i 's/^UsePAM.*/UsePAM no/' /etc/ssh/sshd_config; echo 'root:rootpass123' | chpasswd; kill $(pgrep sshd) 2>/dev/null; sleep 1; /usr/sbin/sshd; sleep 1; pgrep sshd > /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(4)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== sshd status ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))
