"""
Test: use the saved private key to log into the CONTAINER's sshd
But we need to do this from inside the container (since container's 127.0.0.1 is only visible to it)
"""
import urllib.request, json, time, base64
URL = 'http://172.16.0.40:2375'

# Read the private key
with open(r'D:\ch\project\test_id_ed25519', 'r') as f:
    key_content = f.read()
print(f"Key length: {len(key_content)}")

# Use JSCH from a tiny Java test
# Actually, easier: just modify the test to use a different path
# Or: send the key to a running process via stdin

# Wait, the actual issue is:
# - Gateway container at 172.16.0.40:18090
# - SshBridge (in container) tries to connect to host=127.0.0.1 port=22
# - 127.0.0.1 in container = the container itself (where sshd is now running)
# - Should work with the key auth

# So I just need to:
# 1. Put the key in a place where SshBridge can load it
# 2. Or pass it via API

# Simplest: add a "privateKey" field to AuthRequest, store in Connection, use in SshBridge
# But that's a feature. For testing, just use the API to send the key inline.

# Actually wait - the SshBridge currently uses password only. JSCH supports key auth too.
# I'd need to modify SshBridge to load key from a connection field.

# Alternative: send the private key as the "password" field, and have SshBridge try
# password first, then if it starts with "-----BEGIN", use it as a key.

# Even simpler: use PasswordAuthentication but with the correct password
# We set root's password to rootpass123 - let me verify
body = json.dumps({"Cmd": ["sh", "-c", "echo 'rootpass123' | su - root -c 'whoami' 2>&1 > /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== su test ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))

# If su works, then password is correct
# The issue is just JSCH's auth. Maybe JSCH needs pubkey accepted
# Let's check: from inside container, try SSH to 127.0.0.1 with key
body = json.dumps({"Cmd": ["sh", "-c", "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i /root/.ssh/id_ed25519 root@127.0.0.1 'whoami' > /tmp/p.log 2>&1; cat /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(5)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== ssh from inside ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))
