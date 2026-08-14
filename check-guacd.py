"""Check guacd in container via exec"""
import requests, socket, json, time

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_exec(cmd, timeout=30):
    cr = requests.post(f"{DOCKER}/containers/{CONTAINER}/exec",
                       json={"Cmd": cmd, "AttachStdout": True, "AttachStderr": True, "Tty": False},
                       timeout=10)
    cr.raise_for_status()
    cid = cr.json()['Id']
    # start exec
    s = socket.create_connection(("172.16.0.40", 2375), timeout=timeout)
    req = (f'POST /exec/{cid}/start HTTP/1.1\r\nHost: 172.16.0.40:2375\r\n'
           f'Content-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n'
           f'Content-Length: 2\r\n\r\n{{}}').encode()
    s.sendall(req)
    s.settimeout(timeout)
    buf = b''
    while True:
        try:
            c = s.recv(8192)
            if not c:
                break
            buf += c
        except Exception:
            break
    s.close()
    # parse chunks
    pos = 0
    out = ''
    while pos + 8 <= len(buf):
        hdr = buf[pos:pos+8]; pos += 8
        stream = hdr[0]
        size = int.from_bytes(hdr[4:8], 'big')
        if size > 1024*1024:
            break
        chunk = buf[pos:pos+size]; pos += size
        if stream in (1, 2):
            out += chunk.decode('utf-8', errors='replace')
    return out

cmds = [
    ["which", "guacd"],
    ["guacd", "-v"],
    ["ps", "aux"],
    ["netstat", "-tulpn"],
]

for cmd in cmds:
    print(f"\n=== {' '.join(cmd)} ===")
    out = docker_exec(cmd, timeout=10)
    print(out)
