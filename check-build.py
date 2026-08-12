"""
Check what actually happened in guacd build dir
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, timeout=2):
    body = json.dumps({"Cmd": ["sh", "-c", f"{cmd}"]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
        data=body, headers={'Content-Type': 'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    out = urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
        data=b'{"Tty":false,"Detach":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
    time.sleep(timeout)
    return out.decode('utf-8', errors='replace')

print("=== A. source dir contents ===")
print(run("ls -la /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/ 2>&1 | head -20"))

print("\n=== B. configure result? ===")
print(run("cat /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/config.status 2>&1 | head -3"))

print("\n=== C. config.log tail ===")
print(run("tail -25 /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/config.log 2>&1"))

print("\n=== D. Makefile exists? ===")
print(run("ls -la /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/Makefile 2>&1"))

print("\n=== E. src/guacd Makefile ===")
print(run("ls -la /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5/src/guacd/ 2>&1 | head -10"))

print("\n=== F. look for guacd binary ANYWHERE ===")
print(run("find /usr/local /root /opt -name 'guacd*' 2>/dev/null"))

print("\n=== G. re-run configure to see real output ===")
print(run("cd /root/.utils-support-gateway/cache/guacd/1.5.5/guacamole-server-1.5.5 && ./configure --disable-guaclog --disable-doc 2>&1 | tail -15", timeout=120))
