"""
Inspect container network state + thread dump
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

def get(path):
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path={path}', timeout=30)
    data = r.read()
    if len(data) < 512: return ''
    size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8', errors='replace')
    try: size = int(size_str, 8)
    except: size = 0
    return data[512:512+size].decode('utf-8', errors='replace')

# 1. Network state
print('=== /proc/net/tcp ===')
run('cat /proc/net/tcp > /tmp/a.log')
print(get('/tmp/a.log'))

# 2. Java thread dump (find blocked threads)
print('\n=== java threads (kill -3) ===')
run('ps -ef | grep java | grep -v grep | awk \'{print $2}\' > /tmp/pid; cat /tmp/pid > /tmp/a.log')
pid = get('/tmp/a.log').strip()
print(f'java pid: {pid}')

if pid:
    # send SIGQUIT to get thread dump in stdout
    run(f'kill -3 {pid} 2>&1; sleep 2; echo done')
    print('SIGQUIT sent, fetching logs...')
    # get logs via Docker API
    r = urllib.request.urlopen(f'{URL}/containers/gateway-server/logs?stdout=true&stderr=true&tail=300', timeout=10)
    logs = r.read().decode('utf-8', errors='replace')
    # filter lines containing thread refs
    lines = logs.split('\n')
    interesting = [l for l in lines if any(kw in l for kw in ('ws-bridge-reader', 'SSH', 'ChannelShell', 'BLOCKED', 'TIMED_WAITING', 'guacd', 'connect'))]
    print(f'\n--- thread related lines ({len(interesting)}) ---')
    for l in interesting[-40:]:
        print(l[:200])
