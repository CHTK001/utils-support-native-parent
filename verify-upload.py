"""
Verify upload and check if file landed
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# List /tmp
body = json.dumps({"Cmd": ["sh", "-c", "ls -la /tmp > /tmp/p.log 2>&1; echo done"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
size = int(size_str, 8) if size_str else 0
print("=== /tmp contents ===")
print(data[512:512+size].decode('utf-8', errors='replace'))
