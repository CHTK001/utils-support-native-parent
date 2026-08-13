"""
Parse thread dump
"""
import urllib.request, json, sys, time, re
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

r = urllib.request.urlopen(f'{URL}/containers/gateway-server/logs?stdout=true&stderr=true&tail=500', timeout=15)
logs = r.read().decode('utf-8', errors='replace')

clean = re.sub(r'\x00+', '\n', re.sub(r'[^\x20-\x7e\n]', ' ', logs))
lines = clean.split('\n')

# find thread sections
thread_starts = []
for i, l in enumerate(lines):
    if l.startswith('"') and (' #' in l):
        thread_starts.append(i)

print(f'threads found: {len(thread_starts)}')

# show all thread names + states
for ts in thread_starts:
    name = lines[ts].strip()[:80]
    # find first non-name line that has State:
    state = '?'
    frames = []
    for j in range(ts, min(ts+20, len(lines))):
        if 'java.lang.Thread.State:' in lines[j]:
            state = lines[j].split(':')[1].strip().split()[0]
            # next non-empty lines until empty are frames
            k = j+1
            while k < min(ts+30, len(lines)) and lines[k].strip().startswith(('at ', '- ', '"', '~')):
                frames.append(lines[k].strip())
                k += 1
            break
    print(f'{name:60s} {state}')
    for fr in frames[:3]:
        print(f'  {fr[:150]}')
