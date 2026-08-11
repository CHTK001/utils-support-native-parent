"""
Fix sshd: use password auth explicitly, disable PAM
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

body = json.dumps({"Cmd": ["sh", "-c", "sed -i 's/^#PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config; sed -i 's/^UsePAM.*/UsePAM no/' /etc/ssh/sshd_config; sed -i 's/^#PermitEmptyPasswords.*/PermitEmptyPasswords no/' /etc/ssh/sshd_config; grep -iE 'permit|passwordauth|usepam' /etc/ssh/sshd_config > /tmp/p.log 2>&1; kill $(pgrep sshd) 2>&1 >> /tmp/p.log; sleep 1; /usr/sbin/sshd >> /tmp/p.log 2>&1; sleep 1; netstat -tln 2>/dev/null | grep 22 >> /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(10)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print(data[512:512+sz].decode('utf-8', errors='replace'))
