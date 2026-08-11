"""
Use key-based auth: generate key, put in authorized_keys, use key in JSCH
"""
import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

# Generate key for tester
body = json.dumps({"Cmd": ["sh", "-c", "mkdir -p /root/.ssh; ssh-keygen -t ed25519 -N '' -f /root/.ssh/id_ed25519 -q; cat /root/.ssh/id_ed25519.pub >> /root/.ssh/authorized_keys; chmod 600 /root/.ssh/authorized_keys; ls -la /root/.ssh/ > /tmp/p.log 2>&1; cat /root/.ssh/id_ed25519 >> /tmp/p.log 2>&1"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(5)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
content = data[512:512+sz].decode('utf-8', errors='replace')
print(content)
# Save the private key
if 'PRIVATE KEY' in content:
    # Find -----BEGIN... to -----END...
    start = content.find('-----BEGIN')
    end = content.find('-----END', start) + len('-----END OPENSSH PRIVATE KEY-----')
    key = content[start:end]
    with open(r'D:\ch\project\test_id_ed25519', 'w') as f:
        f.write(key)
    print(f"Saved key to D:\\ch\\project\\test_id_ed25519 ({len(key)} bytes)")
