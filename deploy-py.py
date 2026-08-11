import urllib.request, json, time, sys

sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# 1. State
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/json')
d = json.loads(r.read())
s = d['State']
print(f"=== State: Status={s['Status']} Running={s['Running']} Pid={s['Pid']} Started={s['StartedAt'][:19]}")

# 2. Latest logs
print("=== Last 50 lines of logs ===")
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/logs?stdout=true&stderr=true&tail=50')
raw = r.read()
print(raw.decode('utf-8', errors='replace')[-3000:])

# 3. Test HTTP from inside container via exec
body = {
    "Cmd": ["sh", "-c", "wget -qO- http://127.0.0.1:8090/api/connections/list 2>&1; echo; echo ===HOSTNAME===$(hostname); ss -tlnp 2>/dev/null | grep -E '8090|8182|4822' || netstat -tln 2>/dev/null | grep -E '8090|8182'"],
    "AttachStdout": True,
    "AttachStderr": True
}
data = json.dumps(body).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=data, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
exec_id = json.loads(r.read())['Id']
print(f"\n=== exec id: {exec_id} ===")

# Start
req2 = urllib.request.Request(f'{URL}/exec/{exec_id}/start', data=b'{}', headers={'Content-Type': 'application/json'}, method='POST')
urllib.request.urlopen(req2).read()

time.sleep(8)

# Try logs of exec instance (not container)
r3 = urllib.request.urlopen(f'{URL}/exec/{exec_id}/logs?stdout=true&stderr=true')
raw = r3.read()
print(f"=== exec logs size: {len(raw)}")
print(raw.decode('utf-8', errors='replace')[-2000:])

# Also try status
r4 = urllib.request.urlopen(f'{URL}/exec/{exec_id}/json')
d4 = json.loads(r4.read())
print(f"=== exec state: ExitCode={d4['ExitCode']} Running={d4['Running']}")
Output = d4.get('Output')
print(f"Output field: {repr(Output)[:500]}")
