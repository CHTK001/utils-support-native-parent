"""SSH flow with sync/ping to trigger output"""
import requests, socket, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True, "Tty": False},
                       timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    s = socket.create_connection(("172.16.0.40", 2375), timeout=timeout)
    body = json.dumps({"Detach": False}).encode()
    req = (f'POST /exec/{cid}/start HTTP/1.1\r\nHost: 172.16.0.40:2375\r\n'
           f'Content-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n'
           f'Content-Length: {len(body)}\r\n\r\n').encode() + body
    s.sendall(req)
    s.settimeout(timeout)
    buf = b''
    while True:
        try:
            c = s.recv(8192)
            if not c: break
            buf += c
        except Exception:
            break
    s.close()
    parts = buf.split(b'\r\n\r\n', 1)
    body_buf = parts[1] if len(parts) > 1 else b''
    pos = 0; stdout = ''; stderr = ''
    while pos + 8 <= len(body_buf):
        hdr = body_buf[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        chunk = body_buf[pos:pos+size]; pos += size
        text = chunk.decode('utf-8', errors='replace')
        if stream == 1: stdout += text
        elif stream == 2: stderr += text
    return stdout, stderr

test_script = """
python3 << 'EOF'
import socket, time

def enc(v):
    return str(len(v.encode())).encode() + b'.' + v.encode() + b','

def inst(*parts):
    sb = b''
    for p in parts:
        sb += enc(p)
    return sb[:-1] + b';'

def recv_instr(sock, timeout):
    sock.settimeout(timeout)
    buf = b''
    while b';' not in buf:
        try:
            c = sock.recv(4096)
            if not c: break
            buf += c
        except socket.timeout:
            break
    return buf

s = socket.socket()
s.settimeout(8)
s.connect(('127.0.0.1', 4822))

# 1. select
s.sendall(inst('select', 'ssh'))
args = recv_instr(s, 3)
print('1 args:', args[:200])
time.sleep(0.2)

# 2. size
s.sendall(inst('size', '1024', '768', '96'))
time.sleep(0.2)

# 3. connect
connect_parts = ['connect', '1024', '768', '96',
    '127.0.0.1', '', '22', 'root', 'rootpass123',
    'monospace', '12', 'true', '/', 'false', 'false',
    '', '', 'en_US', '', '', 'false',
    '', '', 'false', 'false', 'false', 'false',
    'false', '0', '', 'linux', '10000', 'en_US', '',
    'false', 'false', 'false', '', '', '', '0', '0']
s.sendall(inst(*connect_parts))

# read ready + stream
print('2 waiting ready + output...')
all_data = b''
deadline = time.time() + 6
while time.time() < deadline:
    try:
        chunk = s.recv(65536)
        if not chunk:
            break
        all_data += chunk
    except socket.timeout:
        break

print(f'3 total received: {len(all_data)} bytes')
print('   raw:', all_data[:1000])

# 4. send key input for 'id\\r' via key instruction: key,<pressed>,<keysym>,<name>
# keysym for 'i' = 0x69, 'd'=0x64 ... simpler: use 'keydown' then 'keyup' per char
def send_key(sock, keysym, name):
    # Guacamole key instruction: key,<1=down/0=up>,<keysym>,<name>
    sock.sendall(inst('key', '1', str(keysym), name))
    sock.sendall(inst('key', '0', str(keysym), name))
    time.sleep(0.05)

# type "id\\r" (enter keysym 0xFF0D)
for ch, ks, name in [('i', 0x69, 'i'), ('d', 0x64, 'd'), ('\\r', 0xFF0D, 'Return')]:
    send_key(s, ks, name)

print('4 sent: id\\r')

# read output
out2 = b''
deadline = time.time() + 6
while time.time() < deadline:
    try:
        chunk = s.recv(65536)
        if not chunk: break
        out2 += chunk
    except socket.timeout:
        break

print(f'5 after id: {len(out2)} bytes')
print(out2[:2000])
s.close()
EOF
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=60)
print("STDOUT:")
print(out)
print("STDERR:")
print(err)