import urllib.request, json, sys, time
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'

# Use python -c with urllib (or socket) inside container
cmds = [
    "sh -c \"set -e; echo HOSTNAME=$(hostname); echo === ports ===; ss -tlnp 2>/dev/null; echo; echo === /api/connections/list ===; python3 -c 'import urllib.request; print(urllib.request.urlopen(\\\"http://127.0.0.1:8090/api/connections/list\\\").read().decode())' 2>&1 || echo 'no python3'\""
]

for cmd in cmds:
    print(f"\n=== exec: {cmd[:80]} ===")
    body = json.dumps({
        "Cmd": ["sh", "-c", cmd.replace('"', '\\"')],
        "AttachStdout": True,
        "AttachStderr": True
    }).encode('utf-8')
    req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
    r = urllib.request.urlopen(req)
    exec_id = json.loads(r.read())['Id']

    req2 = urllib.request.Request(f'{URL}/exec/{exec_id}/start', data=b'{}', headers={'Content-Type': 'application/json'}, method='POST')
    urllib.request.urlopen(req2).read()

    time.sleep(6)

    r4 = urllib.request.urlopen(f'{URL}/exec/{exec_id}/json')
    d4 = json.loads(r4.read())
    print(f"ExitCode: {d4['ExitCode']}")
    out = d4.get('Output')
    if out is not None:
        print(out)
    else:
        # Use container logs (won't show exec output but tells status)
        print("(Output null - using /containers/gateway-server/logs instead)")
        r5 = urllib.request.urlopen(f'{URL}/containers/gateway-server/logs?stdout=true&stderr=true&tail=20')
        print(r5.read().decode('utf-8', errors='replace')[-1500:])
