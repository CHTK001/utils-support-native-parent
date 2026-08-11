"""
From container, try to reach 127.0.0.1:22 (its own 127.0.0.1 - won't work)
But the container can reach the SANDBOX through some bridge IP.

Strategy: use docker network gateway 172.18.0.1 (the docker0 bridge, which is the docker host)
But that goes to the Linux host, not the Windows sandbox.

Actually - the gateway container is running on a Linux machine (172.16.0.40).
The Windows sandbox is the host of my Playwright. The user is testing in this sandbox.
The container is REMOTE (on 172.16.0.40).

So for the test to work end-to-end, the SSH target needs to be reachable from 172.16.0.40's container.
172.16.0.40:22 IS reachable from sandbox (we tested).
But what does the CONTAINER see when it tries 172.16.0.40:22?

The container is on Linux machine 172.16.0.40. 172.16.0.40:22 from inside the container means
"the local machine's SSH". The Linux machine at 172.16.0.40 might have its own SSH server.

Let me check: is there a sshd running on the Linux host?
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# Run a Python script in the container to test connections
import io, tarfile
buf = io.BytesIO()
script = '''
import socket
targets = [("127.0.0.1", 22), ("172.18.0.1", 22), ("172.17.0.1", 22), ("172.16.0.40", 22), ("host.docker.internal", 22)]
for h, p in targets:
    try:
        s = socket.socket()
        s.settimeout(3)
        s.connect((h, p))
        print(f"{h}:{p} OPEN")
        s.close()
    except Exception as e:
        print(f"{h}:{p} {type(e).__name__}: {str(e)[:80]}")
'''
with tarfile.open(fileobj=buf, mode='w') as tar:
    data = script.encode('utf-8')
    info = tarfile.TarInfo(name='p.py')
    info.size = len(data)
    tar.addfile(info, io.BytesIO(data))
tar_bytes = buf.getvalue()

req = urllib.request.Request(f'{URL}/containers/gateway-server/archive?path=/tmp',
    data=tar_bytes, method='PUT')
req.add_header('Content-Type', 'application/x-tar')
r = urllib.request.urlopen(req)

# Need python3 or python
body = json.dumps({"Cmd": ["sh", "-c", "which python3 python 2>&1; ls /usr/bin/python* 2>&1; echo --- ; (python3 /tmp/p.py 2>&1 || python /tmp/p.py 2>&1 || echo NO_PY) > /tmp/p.log; echo done"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(8)

# Read
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
size = int(size_str, 8) if size_str else 0
print("=== Container network test ===")
print(data[512:512+size].decode('utf-8', errors='replace'))
