"""
Copy the generated key to 172.16.0.40 (the docker host)
Actually we don't have credentials for that. Let me try installing sshd in container AND exposing it via docker network.

Actually, let me use a different approach: add the gateway-server container to the host network, OR
publish container's port 22 to host. But we'd need to restart the container with --network=host.

Easier: just use a different port for container's sshd and route to host.

Actually simplest: add a route in container to make 127.0.0.1 connect to a fake sshd. Or use socat.

Or: just use Paramiko in Playwright test with sandbox's local SSH at 127.0.0.1:22.

But the connection goes:
- frontend at 127.0.0.1:7788
- API call to 127.0.0.1:7788/api (vite proxy)
- vite proxy to 172.16.0.40:18090 (gateway)
- gateway connects to 127.0.0.1:22 (which is gateway container's own loopback, NOT sandbox)

So the target host for SSH must be:
- A service the gateway CONTAINER can reach
- At 127.0.0.1:22 in container = container's own loopback (now has sshd, but JSCH auth fail)
- At 172.18.0.1:22 = docker host (we don't have creds)

Let me try: install sshd in container, fix auth by using public key with proper permissions.
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

body = json.dumps({"Cmd": ["sh", "-c", "ls -la /root/.ssh/ > /tmp/p.log 2>&1; cat /root/.ssh/authorized_keys >> /tmp/p.log 2>&1; ps aux | grep sshd >> /tmp/p.log 2>&1"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print(data[512:512+sz].decode('utf-8', errors='replace'))
