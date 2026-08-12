"""
Start vite dev server on 7788 in background
"""
import subprocess, time, sys, os, socket
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# Clear cache
import shutil
for path in [r'D:\ch\project\vue-support-parent-starter\apps\vue-support-gateway-starter\node_modules\.vite']:
    if os.path.exists(path):
        shutil.rmtree(path, ignore_errors=True)

# Start vite
log_path = r'D:\ch\project\vite.log'
log_file = open(log_path, 'w', encoding='utf-8')
proc = subprocess.Popen(
    ['node', r'D:\ch\project\vue-support-parent-starter\apps\vue-support-gateway-starter\node_modules\.bin\..\vite\bin\vite.js'],
    cwd=r'D:\ch\project\vue-support-parent-starter\apps\vue-support-gateway-starter',
    stdout=log_file, stderr=subprocess.STDOUT,
    creationflags=0x00000008
)
print(f'  started pid={proc.pid}')

# Wait for ready
for i in range(90):
    time.sleep(2)
    log = open(log_path, encoding='utf-8', errors='replace').read()
    if 'ready in' in log and 'Local:' in log:
        print(f'  ready at {i*2}s')
        break

# Check for errors
log = open(log_path, encoding='utf-8', errors='replace').read()
err_lines = [l for l in log.split('\n') if 'error' in l.lower() or 'fail' in l.lower()]
print('--- error/fail lines ---')
for l in err_lines[-20:]:
    print(f'  {l[:200]}')

# Check
try:
    s = socket.socket(); s.settimeout(2); s.connect(('127.0.0.1', 7788))
    print('  ✓ 7788 OK'); s.close()
except Exception as e:
    print(f'  7788 FAIL: {e}')