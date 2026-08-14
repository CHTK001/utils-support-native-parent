"""Test guacd with multiple protocol variants"""
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

# Try multiple select variants
test_script = """
python3 << 'EOF'
import socket

def test(label, inst):
    try:
        s = socket.socket()
        s.settimeout(4)
        s.connect(('127.0.0.1', 4822))
        s.sendall(inst)
        data = s.recv(4096)
        print(f'{label}: RECV {data!r}')
        s.close()
    except Exception as e:
        print(f'{label}: ERROR {e}')

# Test 1: position args (what v13 sends)
test('pos ssh', b'6.select,3.ssh,9.127.0.0.1,2.22,4.root,11.rootpass123,4.1024,3.768,2.96;')

# Test 2: position args no password
test('pos ssh nopass', b'6.select,3.ssh,9.127.0.0.1,2.22,4.root,0.,4.1024,3.768,2.96;')

# Test 3: vnc
test('pos vnc', b'6.select,3.vnc,9.127.0.0.1,4.5900,0.,0.,4.1024,3.768,2.96;')

# Test 4: minimal (just protocol)
test('min ssh', b'6.select,3.ssh;')

# Test 5: key-value (what v12 sent)
test('kv ssh', b'6.select,3.ssh,4.host,9.127.0.0.1,4.port,2.22,8.username,4.root,8.password,11.rootpass123;')

# Test 6: list guacd supported protocols by sending bad proto
test('bad proto', b'6.select,3.foo;')
EOF
"""
out, err = docker_exec_sync(["sh", "-c", test_script], timeout=30)
print("STDOUT:")
print(out)
print("STDERR:")
print(err)
