"""
Check what's in cache + whether bootstrap triggered
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, timeout=5):
    body = json.dumps({"Cmd": ["sh", "-c", f"{cmd} > /tmp/p.log 2>&1"]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
        data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)

def read(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=10)
    data = r.read()
    sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
    return data[512:512+sz].decode('utf-8', errors='replace')

# List cache thoroughly
run("find /root/.utils-support-gateway -maxdepth 5 2>/dev/null > /tmp/p.log")
print("=== cache tree ===")
print(read("/tmp/p.log"))

# Check if there's any guacd binary
run("find / -name 'guacd' -type f 2>/dev/null > /tmp/p.log")
print("\n=== guacd binaries on disk ===")
print(read("/tmp/p.log"))

# Check gcc/make availability
run("which gcc make autoconf 2>&1 > /tmp/p.log")
print("\n=== compile tools ===")
print(read("/tmp/p.log"))

# Check tarball content
run("tar tzf /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5.tar.gz 2>&1 | head -5 > /tmp/p.log")
print("\n=== tarball contents (first 5) ===")
print(read("/tmp/p.log"))
