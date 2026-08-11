import urllib.request, json, time
URL = 'http://172.16.0.40:2375'
body = json.dumps({"Cmd": ["sh", "-c", "rm -f /root/.utils-support-gateway/gateway.db > /tmp/p.log 2>&1; echo done"]}).encode()
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec', data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(f'{URL}/exec/{eid}/start', data=b'{"Tty":false}', headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(2)
print('DB deleted')
