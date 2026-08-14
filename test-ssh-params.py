"""Test SSH select with various param styles to find what works"""
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
import socket, time

def test(label, inst, expect_connect=False):
    try:
        s = socket.socket()
        s.settimeout(6)
        s.connect(('127.0.0.1', 4822))
        s.sendall(inst)
        time.sleep(0.5)
        data = s.recv(4096)
        print(f'{label}: RECV {data[:300]!r}')
        s.close()
    except Exception as e:
        print(f'{label}: ERROR {e}')

# Style 1: what guacamole-common-js sends: select ssh,<host>,<port>,<username>,<password>,... 
# guacd accepts: select <protocol>,<host>,<port>,<username>,<password> (positional)
test('ssh pos5', b'6.select,3.ssh,9.127.0.0.1,2.22,4.root,11.rootpass123;')

# Style 2: with args instruction using key=value (guacd ssh expects this form)
# Format: select ssh,<key>=<value>... -- but guacd actually wants: select,ssh,... 
# After args are received, client sends "size" then "connect", then it connects.
# For the select itself: guacd wants select with connection params.
# Actually, for SSH, guacd requires: select,ssh,hostname,127.0.0.1,port,22,username,root,password,xxx
test('ssh kv hostname', b'6.select,3.ssh,8.hostname,9.127.0.0.1,4.port,2.22,8.username,4.root,8.password,11.rootpass123;')

# Style 3: with args wrapped - send args first then size then connect
# Real flow: select ssh -> get args -> send "size" -> send "connect" -> connection established
# Test the full flow
def full_flow(label):
    try:
        s = socket.socket()
        s.settimeout(8)
        s.connect(('127.0.0.1', 4822))
        # 1. select ssh
        s.sendall(b'6.select,3.ssh;')
        time.sleep(0.3)
        args = s.recv(8192)
        print(f'{label} select->args: {args[:200]!r}')
        # 2. send size
        s.sendall(b'4.size,4.1000,4.750,1.96;')
        time.sleep(0.3)
        try:
            d = s.recv(8192)
            print(f'{label} size->recv: {d[:200]!r}')
        except socket.timeout:
            print(f'{label} size->timeout')
        # 3. send connect with params
        # format: connect,<width>,<height>,<dpi>,<host>,<port>,<username>,<password>
        connect_inst = b'7.connect,4.1000,4.750,1.96,9.127.0.0.1,2.22,4.root,11.rootpass123;'
        s.sendall(connect_inst)
        time.sleep(1)
        try:
            d = s.recv(8192)
            print(f'{label} connect->recv: {d[:300]!r}')
        except socket.timeout:
            print(f'{label} connect->timeout')
        s.close()
    except Exception as e:
        print(f'{label}: ERROR {e}')

full_flow('full')
EOF
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=60)
print("STDOUT:")
print(out)
print("STDERR:")
print(err)
