"""
Check container state
"""
import urllib.request, json, sys, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, wait=10):
    body = json.dumps({'Cmd':['sh','-c',cmd]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type':'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false,"Detach":true}', headers={'Content-Type':'application/json'}, method='POST')).read()
    for i in range(wait*10):
        time.sleep(0.1)
        ins = json.loads(urllib.request.urlopen(f'{URL}/exec/{eid}/json').read())
        if not ins.get('Running'): break
    return ins

def get(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=30)
    data = r.read()
    if len(data) < 512: return ''
    size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8', errors='replace')
    try: size = int(size_str, 8)
    except: size = 0
    return data[512:512+size].decode('utf-8', errors='replace')

print('=== ps ===')
run('ps -ef 2>&1 > /tmp/p.log')
print(get('/tmp/p.log'))

print('\n=== ports ===')
run('ss -tlnp 2>&1 > /tmp/p.log')
print(get('/tmp/p.log'))

print('\n=== recent log ===')
log = urllib.request.urlopen(f'{URL}/containers/gateway-server/logs?stdout=true&stderr=true&tail=200', timeout=10).read().decode('utf-8', errors='replace')
import re
clean = re.sub(r'\x00+', '\n', re.sub(r'[^\x20-\x7e\n]', ' ', log))
print(clean[-3000:])