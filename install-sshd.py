"""
Use the raw TCP bridge path (WsBridgeServer:142) instead of SshBridge
to verify WS endpoint + tunnel logic works end-to-end.

Architecture:
- frontend ws://host:18182/ws/{tunnelId}
- server accepts WS, opens raw TCP to target, bridges bytes

But this requires creating a tunnel via /api/connections/authenticate first.

Let me read what /authenticate does:
1. resolveConnection -> returns Connection
2. tunnelRegistry.open(conn) -> creates tunnel + bridge
3. for SSH protocol, calls SshProtocolServerFactory -> SshBridge -> JSCH

So I can't avoid JSCH for SSH. The only way to test WS endpoint is to:
- Modify code to skip JSCH connect (test only)
- Or deploy an SSH server in the container
- Or find another way

Let me just deploy sshd in the container temporarily, OR
- Use a real public SSH server (like ssh.gitlab.com, or shell.cloud.google.com)
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# Try installing sshd in container
URL = 'http://172.16.0.40:2375'
body = json.dumps({"Cmd": ["sh", "-c", "apt-get update 2>&1 | tail -5; apt-get install -y openssh-server 2>&1 | tail -5; which sshd > /tmp/p.log 2>&1; ls /usr/sbin/sshd >> /tmp/p.log 2>&1; echo done"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{"Tty":false}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(60)  # install takes time

r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
size = int(size_str, 8) if size_str else 0
print("=== apt-get sshd ===")
print(data[512:512+size].decode('utf-8', errors='replace'))
