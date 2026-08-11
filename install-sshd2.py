import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# Run apt-get with output to file
body = json.dumps({"Cmd": ["sh", "-c", "apt-get install -y openssh-server 2>&1 | tail -20 > /tmp/p.log; echo INSTALLED > /tmp/done; which sshd >> /tmp/p.log 2>&1; ls /usr/sbin/sshd >> /tmp/p.log 2>&1"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{"Tty":false}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(90)  # install can take 30-60s

# Check if done
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/done', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("done file:", data[512:512+sz].decode('utf-8', errors='replace'))

# Read p.log
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("p.log:", data[512:512+sz].decode('utf-8', errors='replace'))
