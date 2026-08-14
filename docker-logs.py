"""Get gateway-server logs from container via Docker API"""
import requests, socket, json

DOCKER = "http://172.16.0.40:2375"
CONTAINER = "gateway-server"

def docker_logs(tail=100):
    r = requests.get(f"{DOCKER}/containers/{CONTAINER}/logs",
                     params={"stdout": "1", "stderr": "1", "tail": str(tail)},
                     timeout=15)
    return r.content

raw = docker_logs(200)
# Docker log stream: 8-byte header per line
pos = 0
out_lines = []
while pos + 8 <= len(raw):
    hdr = raw[pos:pos+8]
    pos += 8
    stream = hdr[0]
    size = int.from_bytes(hdr[4:8], 'big')
    chunk = raw[pos:pos+size]
    pos += size
    try:
        text = chunk.decode('utf-8', errors='replace')
    except Exception:
        continue
    if stream in (1, 2):
        out_lines.append(text)

for line in out_lines:
    if any(k in line.lower() for k in ['select', 'guac', 'ssh', 'bridge', 'error', 'ws', 'tunnel', 'connect', 'exception', 'fail']):
        print(line.rstrip())
