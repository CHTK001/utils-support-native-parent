"""
Test SSH login from inside container via JSCH (since we have jrt-fs + the gateway jar)
Actually simpler: just ssh to 127.0.0.1:22 with paramiko via container's java
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

# Test: ssh with -o StrictHostKeyChecking=no -o PreferredAuthentications=password
body = json.dumps({"Cmd": ["sh", "-c", "(which ssh; ls /usr/bin/ssh 2>&1) > /tmp/p.log; ssh -V 2>&1 >> /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(5)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print(data[512:512+sz].decode('utf-8', errors='replace'))
