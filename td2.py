"""
Get full thread dump
"""
import urllib.request, json, sys, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

def run(cmd, wait=5):
    body = json.dumps({'Cmd':['sh', '-c', cmd]}).encode()
    r = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body,
                              headers={'Content-Type': 'application/json'}, method='POST')
    eid = json.loads(urllib.request.urlopen(r).read())['Id']
    urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start',
        data=b'{"Tty":false,"Detach":true}',
        headers={'Content-Type': 'application/json'}, method='POST')).read()
    for i in range(wait * 10):
        time.sleep(0.1)
        ins = json.loads(urllib.request.urlopen(f'{URL}/exec/{eid}/json').read())
        if not ins.get('Running'): break
    return ins

# Trigger SIGQUIT and wait
run('kill -3 1; sleep 2; echo done')
time.sleep(2)

# Get full logs
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/logs?stdout=true&stderr=true&tail=500', timeout=15)
logs = r.read().decode('utf-8', errors='replace')

# Find thread dump section
import re
clean = re.sub(r'\x00+', '\n', re.sub(r'[^\x20-\x7e\n]', ' ', logs))
# extract the "JNI system thread" or thread dumps
lines = clean.split('\n')
print(f'total lines: {len(lines)}')
# find lines starting with '"' that indicate thread names
for i, l in enumerate(lines):
    if '"main"' in l or '"ws-bridge-reader' in l or '"ws-accept"' in l or '"JSCH' in l or '"sshd' in l:
        # print context
        start = max(0, i-1)
        end = min(len(lines), i+5)
        for j in range(start, end):
            print(f'  {lines[j][:200]}')
        print('---')
