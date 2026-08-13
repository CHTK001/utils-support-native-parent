"""
Verify v9: processes, ports, HTTP API, SSH via WS e2e
"""
import urllib.request, json, time, sys, socket, os, struct
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, wait=10):
    body = json.dumps({'Cmd': ['sh', '-c', cmd]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body,
                               headers={'Content-Type': 'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
        data=b'{"Tty":false,"Detach":true}', headers={'Content-Type': 'application/json'},
        method='POST')).read()
    for i in range(wait * 10):
        time.sleep(0.1)
        ins = json.loads(urllib.request.urlopen(f'{URL}/exec/{eid}/json').read())
        if not ins.get('Running'):
            break
    return ins

def get(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=30)
    d = r.read()
    if len(d) < 512:
        return ''
    sz = int(d[124:136].rstrip(b'\x00').rstrip(b' ').decode(), 8)
    return d[512:512 + sz].decode('utf-8', errors='replace')

print('=== ps ===')
run('ps -ef > /tmp/a.log')
print(get('/tmp/a.log'))

print('\n=== ports ===')
run('ss -tln > /tmp/a.log')
print(get('/tmp/a.log'))

print('\n=== HTTP API ===')
run('curl -sS -m 5 http://127.0.0.1:8080/api/connections/list > /tmp/a.log')
print(get('/tmp/a.log'))