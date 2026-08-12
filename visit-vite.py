"""
Force vite re-optimize by visiting root + waiting
"""
import urllib.request, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# Trigger vite optimizer by fetching / which will discover deps
for i in range(20):
    try:
        r = urllib.request.urlopen('http://127.0.0.1:7788/', timeout=10)
        print(f'  T+{i*3}s: HTTP {r.status}, body={r.read()[:200]!r}')
        if b'<script' in r.read(2000):
            print('  ✓ HTML has script tags')
        break
    except Exception as e:
        print(f'  T+{i*3}s: {e}')
    time.sleep(3)

# Wait for optimize
time.sleep(10)
try:
    r = urllib.request.urlopen('http://127.0.0.1:7788/', timeout=10)
    body = r.read().decode('utf-8', errors='replace')
    print(f'  body: {body[:500]}')
except Exception as e:
    print(f'  fail: {e}')