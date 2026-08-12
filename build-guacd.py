"""
Install gcc + guacd build deps INSIDE container, then restart gateway
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, timeout=2):
    body = json.dumps({"Cmd": ["sh", "-c", f"{cmd} > /tmp/p.log 2>&1"]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
        data=b'{"Tty":false,"Detach":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)

def read(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=10)
    data = r.read()
    sz = int(data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8'), 8) if data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8') else 0
    return data[512:512+sz].decode('utf-8', errors='replace')

# 1. Install build dependencies
print("=== 1. apt-get install gcc + guacd build deps (2-3 min) ===")
run("apt-get update -qq 2>&1 | tail -3 && apt-get install -y --no-install-recommends "
    "gcc make pkg-config libssh2-1-dev libssl-dev libvncserver-dev libwebsockets-dev "
    "libpango1.0-dev libcairo2-dev libjpeg-dev libpng-dev libossp-uuid-dev "
    "2>&1 | tail -5", timeout=180)
print(read("/tmp/p.log"))

# 2. Check gcc + make
print("\n=== 2. Verify gcc + make ===")
run("which gcc make pkg-config 2>&1 > /tmp/p.log")
print(read("/tmp/p.log"))

# 3. Run configure
print("\n=== 3. ./configure (1-2 min) ===")
run("cd /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5 "
    "&& ./configure --disable-guaclog --disable-doc 2>&1 | tail -8", timeout=180)
print(read("/tmp/p.log"))

# 4. Build
print("\n=== 4. make -j$(nproc) (5-10 min) ===")
run("cd /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5 "
    "&& make -j$(nproc) 2>&1 | tail -5", timeout=600)
print(read("/tmp/p.log"))

# 5. Install
print("\n=== 5. make install ===")
run("cd /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5 "
    "&& make install 2>&1 | tail -5", timeout=60)
print(read("/tmp/p.log"))

# 6. Check guacd binary
print("\n=== 6. guacd binary ===")
run("find / -name guacd -type f 2>/dev/null > /tmp/p.log")
print(read("/tmp/p.log"))

# 7. ldconfig
print("\n=== 7. ldconfig ===")
run("ldconfig; ls /usr/local/lib/ | grep -i guac > /tmp/p.log 2>&1")
print(read("/tmp/p.log"))
