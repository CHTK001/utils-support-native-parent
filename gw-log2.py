import urllib.request, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
r = urllib.request.urlopen('http://172.16.0.40:2375/containers/gateway-server/logs?stdout=true&stderr=true&tail=500')
text = r.read().decode('utf-8', errors='replace')
# Print lines containing 鉴权 or 收到
for line in text.splitlines():
    if "鉴权" in line or "收到" in line or "authenticate" in line or "user=" in line or "target=" in line:
        print(line)
