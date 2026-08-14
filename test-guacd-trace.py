"""Start guacd directly and test SSH"""
import requests, socket, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec_sync(cmd, timeout=60):
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

# guacd valid levels: trace, debug, info, warning, error. Try -L trace
print("=== start guacd trace on 4822 ===")
out, err = docker_exec_sync(["sh", "-c",
    "(nohup /usr/local/sbin/guacd -b 0.0.0.0 -p /tmp/guacd.pid -l 4822 -L trace > /tmp/guacd-trace.log 2>&1 &); "
    "sleep 1.5; ss -tlnp 2>&1 | grep 4822; echo ---LOG---; cat /tmp/guacd-trace.log 2>&1"], timeout=15)
print(out)

print("\n=== test SSH on 4822 ===")
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

s = socket.socket()
s.settimeout(6)
s.connect(('127.0.0.1', 4822))
s.sendall(inst('select', 'ssh'))
args = s.recv(65536)
print('1 args:', args[:150])
s.sendall(inst('size', '1024', '768', '96'))
time.sleep(0.2)
connect_parts = ['connect', '1024', '768', '96',
    '127.0.0.1', '', '22', 'root', 'rootpass123',
    'monospace', '12', 'true', '/', 'false', 'false',
    '', '', 'en_US', '', '', 'false', '', '',
    'false', 'false', 'false', 'false', 'false', '0', '',
    'linux', '10000', 'en_US', '', 'false', 'false', 'false',
    '', '', '', '0', '0']
s.sendall(inst(*connect_parts))
time.sleep(2)
try:
    data = s.recv(65536)
    print('2 after connect:', data[:500])
except socket.timeout:
    print('2 after connect: timeout')
s.close()
EOF
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=30)
print("STDOUT:")
print(out)
print("STDERR:")
print(err)

print("\n=== guacd trace log ===")
out, err = docker_exec_sync(["sh", "-c", "tail -50 /tmp/guacd-trace.log 2>&1"], timeout=10)
print(out)