import urllib.request
import sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
r = urllib.request.urlopen('http://172.16.0.40:2375/containers/gateway-server/logs?stdout=true&stderr=true&tail=500')
text = r.read().decode('utf-8', errors='replace')
for line in text.splitlines():
    if 'gateway-server' in line or 'guacd' in line.lower() or '已就绪' in line or 'download' in line.lower() or '启动' in line:
        print(line[:300])
