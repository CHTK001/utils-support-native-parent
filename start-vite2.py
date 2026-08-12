"""
Start vite dev server with detached process
"""
import subprocess, time, sys, os, socket
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# Clear cache
import shutil
cache = r'D:\ch\project\vue-support-parent-starter\apps\vue-support-gateway-starter\node_modules\.vite'
if os.path.exists(cache):
    shutil.rmtree(cache, ignore_errors=True)

# Use subprocess.Popen with DETACHED_PROCESS so it survives
log_path = r'D:\ch\project\vite.log'
log_file = open(log_path, 'w', encoding='utf-8')

# Use start /B via cmd
cmd = 'start /B "vite" cmd /c "node node_modules\\.bin\\..\\vite\\bin\\vite.js"'
result = subprocess.Popen(
    cmd,
    cwd=r'D:\ch\project\vue-support-parent-starter\apps\vue-support-gateway-starter',
    shell=True,
    stdout=log_file,
    stderr=subprocess.STDOUT,
    creationflags=0x00000008
)
print(f'  spawned pid={result.pid}')

# Wait
for i in range(60):
    time.sleep(2)
    try:
        s = socket.socket(); s.settimeout(1); s.connect(('127.0.0.1', 7788)); s.close()
        print(f'  ✓ 7788 listening at {(i+1)*2}s')
        break
    except:
        pass
else:
    print('  7788 not up after 120s')

# Log tail
log = open(log_path, encoding='utf-8', errors='replace').read()
print('--- vite log (last 1500) ---')
print(log[-1500:])