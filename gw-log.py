"""
Get gateway container log
"""
import urllib.request, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

r = urllib.request.urlopen('http://172.16.0.40:2375/containers/gateway-server/logs?stdout=true&stderr=true&tail=200')
data = r.read()
text = data.decode('utf-8', errors='replace')
# Find lines with SSH/error/隧道/失败
import re
# Just print last 3000 chars
print(text[-3000:])
