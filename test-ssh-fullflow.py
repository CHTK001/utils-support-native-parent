"""Full guacamole SSH flow with proper connect params"""
import requests, socket, json

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
import socket, time, json

# Guacamole protocol (from guacamole-common-js):
# 1. select <protocol> ; -> guacd replies with args instruction listing available params
# 2. client sends: size,<w>,<h>,<dpi> ;  (must come BEFORE connect)
# 3. guacd replies: ready,<id>,<protocol>,<width>,<height> ;  (for RDP/guacd session) OR
#    for ssh it might reply differently
# 4. client sends: connect,<w>,<h>,<dpi>,<...parameters from args in order...> ;
# 5. guacd connects to ssh and starts streaming terminal output

# The SSH args list from earlier:
# VERSION_1_5_0, hostname, host-key, port, username, password, font-name, font-size,
# enable-sftp, sftp-root-directory, sftp-disable-download, sftp-disable-upload,
# private-key, passphrase, color-scheme, command, typescript-path, typescript-name,
# create-typescript-path, recording-path, recording-name, recording-exclude-output,
# recording-exclude-mouse, recording-include-keys, create-recording-path, read-only,
# server-alive-interval, backspace, terminal-type, scrollback, locale, timezone,
# disable-copy, disable-paste, wol-send-packet, wol-mac-addr, wol-broadcast-addr,
# wol-udp-port, wol-wait-time

def enc(v):
    return str(len(v.encode())).encode() + b'.' + v.encode() + b','

def inst(*parts):
    sb = b''
    for p in parts:
        sb += enc(p)
    return sb[:-1] + b';'  # replace last , with ;

s = socket.socket()
s.settimeout(10)
s.connect(('127.0.0.1', 4822))

# Step 1: select ssh
print('SEND select')
s.sendall(inst('select', 'ssh'))
time.sleep(0.5)
args = s.recv(65536)
print('RECV args:', args[:600])
print()

# Step 2: size
print('SEND size')
s.sendall(inst('size', '1024', '768', '96'))
time.sleep(0.5)
try:
    d = s.recv(65536)
    print('RECV after size:', d[:300])
except socket.timeout:
    print('after size: timeout')

# Step 3: connect (SSH params in order from args)
print('SEND connect')
connect_parts = ['connect', '1024', '768', '96',
    '127.0.0.1', '', '22', 'root', 'rootpass123',
    'monospace', '12', 'true', '/', 'false', 'false',
    '', '', 'en_US', '', '', 'false',
    '', '', 'false', 'false', 'false', 'false',
    'false', '0', '', 'linux', '10000', 'en_US', '',
    'false', 'false', 'false', '', '', '', '0', '0']
s.sendall(inst(*connect_parts))
time.sleep(2)
try:
    d = s.recv(65536)
    print('RECV after connect:', d[:500])
except socket.timeout:
    print('after connect: timeout')

s.close()
EOF
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=60)
print("STDOUT:")
print(out)
print("STDERR:")
print(err)
