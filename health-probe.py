import urllib.request, json, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

URL = 'http://172.16.0.40:2375'

# 1. Container state
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/json')
d = json.loads(r.read())
s = d['State']
print(f"Container state: Status={s['Status']} Running={s['Running']} Pid={s['Pid']}")

# 2. Health check via internal exec (inside container)
body = {
    "Cmd": ["sh", "-c", "echo 'health probe:'; echo '--- /api/connections/list ---'; wget -qO- http://127.0.0.1:8090/api/connections/list 2>&1; echo; echo '--- /api/connections/keys ---'; wget -qO- http://127.0.0.1:8090/api/connections/keys 2>&1; echo; echo '--- listening ports ---'; ss -tlnp 2>/dev/null || netstat -tln 2>/dev/null"],
    "AttachStdout": True,
    "AttachStderr": True
}
data = json.dumps(body).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=data, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
exec_id = json.loads(r.read())['Id']

# Start
req2 = urllib.request.Request(f'{URL}/exec/{exec_id}/start', data=b'{}', headers={'Content-Type': 'application/json'}, method='POST')
urllib.request.urlopen(req2).read()

import time
time.sleep(8)

# Get exec logs from the running gateway
# Hmm - /exec/{id}/logs 404. Use /containers/{id}/logs instead with same id
# Actually exec IS its own container-like thing - logs at /exec/{id}/logs requires TTY
# Let's check state for the exec
r4 = urllib.request.urlopen(f'{URL}/exec/{exec_id}/json')
d4 = json.loads(r4.read())
print(f"exec state: ExitCode={d4['ExitCode']} Running={d4['Running']}")
print(f"Output present: {d4.get('Output') is not None}")
