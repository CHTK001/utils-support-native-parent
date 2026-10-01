#!/usr/bin/env python3
"""Linux 电池取值验收：用 bind mount 伪造 `/sys/class/power_supply`。

## 为什么需要这个夹具

`battery.list` 的取值分支（电量百分比、剩余时间换算、状态映射）在
**无电池设备**上永远走不到 —— 台式机与容器都直接返回空列表，CI 上
四个平台全是这种机器，于是"取值正确性"一直是未验项。

Linux 的 `/sys/class/power_supply` 在容器/虚拟机里通常是个空的
tmpfs 挂载点。sysfs 本身不可写，但**挂载点本身可以被覆盖**：

    mount --bind <可控目录> /sys/class/power_supply

这样就能提供内容完全可控的 `type` / `capacity` / `status` /
`energy_now` / `energy_full` / `power_now` / `time_to_empty_now`，
从而验证取值逻辑本身，而不只是"无电池 -> 空列表"。

## 前置条件

- Linux 主机，可 `sudo`（需要 mount 权限）
- `python3`
- `paramiko`（本机侧）
- 被测 `libsysinformer.so`（用**入库产物**，不要用本地重建件）

## 用法

两种模式，**断言逻辑完全相同**（同一段校验脚本被复用，不存在两份实现）：

### 远端模式（SSH 到 Linux 主机）

    export SI_SSH_PASSWORD='...'
    export SI_SO_PATH='<本地 libsysinformer.so 路径>'
    python battery_value_linux.py <host> <user> /tmp/sysinf/libsysinformer.so

密码只从环境变量读，不落盘、不设默认值。

### 本地模式（CI 用，sudo 免密）

    sudo -n python battery_value_linux.py --local <libsysinformer.so>

CI 的 Linux runner 本身就是 Linux 且 `sudo` 免密，可以直接在 runner 上
bind mount 夹具，不需要连任何外部主机 —— 电池取值验收因此从一次性的
人工验证升级为**常驻 CI 门禁**。

## 判定

三个用例全绿且顺序符合契约时打印 `BATTERY_VALUE_OK` / `KALI_BATTERY_VALUE_OK`，
退出码 0；否则打印对应的 `..._FAILED`，退出码 1。

**顺序也是被断言的一部分**：三平台均约定按 `name` 升序。Linux 侧
曾经漏了排序，`read_dir` 的返回顺序由文件系统决定（ext4 哈希序、
tmpfs 插入序），实测同一目录 `ls -U` 给的是 `USB AC BAT1 BAT0`，
即 BAT1 排在 BAT0 之前。这类缺陷不崩不报错，只让列表顺序在
不同挂载/重启间漂移，调用方无法依赖下标。
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import posixpath
import sys
import time

HOST = "192.168.50.198"
USER = "kali"
REMOTE_DIR = "/tmp/sysinf"

# ---------------------------------------------------------------- 远端用例

# 用例内容用**可计算的期望值**，不写魔法数字：
#   capacity=87                     -> percentage = 87.0
#   status=Discharging              -> state = discharging
#   energy_now=18Wh, power_now=10W  -> time_to_empty = 18/10*3600 = 6480
#   energy_full=60Wh                -> time_to_full  = (60-18)/10*3600 = 15120
CASES = [
    {
        "label": "字段齐全",
        "entries": {
            "BAT0": {
                "type": "Battery",
                "model_name": "BAT0-model",
                "capacity": "87",
                "status": "Discharging",
                "energy_now": "18000000",
                "energy_full": "60000000",
                "power_now": "10000000",
            },
            # 非电池条目：必须被过滤掉，否则 count 会不对
            "AC": {"type": "Mains"},
        },
        "count": 1,
        "expect": [{
            "name": "BAT0-model",      # 取 model_name 内容而非目录名
            "percentage": 87.0,         # <- capacity
            "state": "discharging",     # <- status
            "time_to_empty_sec": 6480,  # <- energy_now/power_now*3600
            "time_to_full_sec": 15120,  # <- (energy_full-energy_now)/power_now*3600
        }],
    },
    {
        # 无 power_now：必须退回内核给的 time_to_empty_now，而不是编造或归零
        "label": "缺 power_now 退回内核值",
        "entries": {
            "BAT0": {
                "type": "Battery",
                "model_name": "BAT0-model",
                "capacity": "87",
                "status": "Discharging",
                "time_to_empty_now": "5400",
            },
        },
        "count": 1,
        "expect": [{
            "name": "BAT0-model",
            "percentage": 87.0,
            "state": "discharging",
            "time_to_empty_sec": 5400,
        }],
    },
    {
        # 多电池 + 状态别名 + 非电池过滤；顺序按 name 升序（B0 < B1）
        "label": "多电池与状态别名",
        "entries": {
            "BAT0": {
                "type": "Battery", "model_name": "B0", "capacity": "50",
                "status": "Charging", "energy_now": "10000000",
                "energy_full": "20000000", "power_now": "10000000",
            },
            "BAT1": {
                "type": "Battery", "model_name": "B1", "capacity": "30",
                "status": "Full", "energy_now": "20000000",
                "power_now": "10000000",
                # 无 energy_full -> time_to_full 必须是 None，不能编造
            },
            "AC": {"type": "Mains"},
            "USB": {"type": "USB"},
        },
        "count": 2,
        "expect": [
            {
                "name": "B0", "percentage": 50.0, "state": "charging",
                "time_to_empty_sec": 3600, "time_to_full_sec": 3600,
            },
            {
                "name": "B1", "percentage": 30.0, "state": "full",
                "time_to_empty_sec": 7200, "time_to_full_sec": None,
            },
        ],
    },
]

# 远端执行的校验脚本（纯 ASCII，内联进 heredoc）
REMOTE_VERIFY = r'''
import ctypes, json, sys

lib = ctypes.CDLL(SO_PATH)
lib.sysinformer_call.restype = ctypes.c_void_p
lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]


def call(op, a=None):
    p = lib.sysinformer_call(op.encode(), json.dumps(a or {}).encode())
    r = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    lib.sysinformer_free_string(p)
    return json.loads(r)


cases = json.load(open(CASES_PATH))
ok_all = True

for case in cases:
    env = call("battery.list")
    snap = call("system.snapshot")
    got = env.get("data") or []
    print("--- %s ---" % case["label"])
    print("  battery.list ok=%s  count=%d (expect %d)"
          % (env.get("ok"), len(got), case["count"]))
    same = snap["data"].get("batteries") == got
    print("  snapshot.batteries consistent = %s" % same)
    ok_all = ok_all and same

    if len(got) != case["count"]:
        ok_all = False
        continue

    # 顺序契约：三平台均按 name 升序（见本文件 docstring）
    names = [b.get("name") for b in got]
    print("  %s order: %r sorted=%s"
          % ("ok  " if names == sorted(names) else "FAIL",
             names, names == sorted(names)))
    ok_all = ok_all and names == sorted(names)

    # 按 name 匹配而非按下标：顺序问题上面已单独断言，
    # 这里再按下标匹配会把"顺序错"和"取值错"两种失败混在一起，无法定位
    byname = {b.get("name"): b for b in got}
    for i, exp in enumerate(case["expect"]):
        nm = exp["name"]
        b = byname.get(nm)
        if b is None:
            print("  FAIL [%d].name: %r missing, got %r" % (i, nm, sorted(byname)))
            ok_all = False
            continue
        for k, want in exp.items():
            g = b.get(k)
            if want is None:
                good = g is None
            elif isinstance(want, bool):
                good = g is want
            elif isinstance(want, (int, float)):
                good = g is not None and abs(float(g) - float(want)) <= \
                    max(abs(float(want)) * 0.02, 1.0)
            else:
                good = g == want
            print("  %s [%d].%s: got=%r want=%r"
                  % ("ok  " if good else "FAIL", i, k, g, want))
            ok_all = ok_all and good

print("BATTERY_VALUE_OK" if ok_all else "BATTERY_VALUE_FAILED")
sys.exit(0 if ok_all else 1)
'''


# ---------------------------------------------------------------- 本机侧工具


def shq(s: str) -> str:
    """单引号包裹，供远端 sh -c 使用。"""
    return "'" + s.replace("'", "'\\''") + "'"


class SshShell:
    """把 SSH 会话包装成与 LocalShell 相同的接口。"""

    def __init__(self, run, sudo):
        self.run = run
        self.sudo = sudo


class LocalShell:
    """本机 shell（CI 用）。CI 的 Linux runner 上 `sudo` 是免密的。"""

    def __init__(self, sudo_flag="-n"):
        self.sudo_flag = sudo_flag

    def run(self, cmd, timeout=180):
        import subprocess
        p = subprocess.run(["/bin/sh", "-c", cmd], capture_output=True,
                           timeout=timeout)
        return (p.stdout.decode("utf-8", "replace"),
                p.stderr.decode("utf-8", "replace"))

    def sudo(self, cmd, timeout=180):
        return self.run(f"sudo {self.sudo_flag} sh -c {shq(cmd)}", timeout)


def run_cases(shell, so_path: str, work_dir: str, marker: str,
              pwd_hint: bool = False) -> bool:
    """跑全部用例。SSH 与本地两种模式共用，断言逻辑只有这一份。

    :param pwd_hint: SSH 模式下 `sudo` 会提示输密码，stderr 里的
        "password for" 不是错误，需要忽略。
    """
    def mount_fixture(entries: dict) -> tuple:
        script = "set -e\nFAKE=/tmp/fakebat\nrm -rf $FAKE\nmkdir -p $FAKE\n"
        for d, files in entries.items():
            script += f"mkdir -p $FAKE/{d}\n"
            for fn, val in files.items():
                script += f"echo {shq(val)} > $FAKE/{d}/{fn}\n"
        script += "mount --bind $FAKE /sys/class/power_supply\n"
        return shell.sudo(script)

    cases_path = posixpath.join(work_dir, "bat_cases.json")
    verify_path = posixpath.join(work_dir, "verify_bat.py")
    shell.run(f"mkdir -p {work_dir}")
    # 校验脚本与本文件同源，SSH/本地跑的是**同一份断言**
    shell.run(f"cat > {verify_path} <<'PYEOF'\n"
              f"SO_PATH = {so_path!r}\n"
              f"CASES_PATH = {cases_path!r}\n"
              f"{REMOTE_VERIFY}\nPYEOF")

    all_ok = True
    try:
        for n, case in enumerate(CASES, 1):
            print(f"\n=== case {n}: {case['label']} ===")
            shell.sudo("umount /sys/class/power_supply 2>/dev/null; true")
            _, e = mount_fixture(case["entries"])
            if e.strip() and not (pwd_hint and "password for" in e):
                print(f"  [stderr] {e[:200]}")
            shell.run(f"cat > {cases_path} <<'JEOF'\n"
                      f"{json.dumps([case], ensure_ascii=False)}\nJEOF")
            o, e = shell.run(f"python3 {verify_path}")
            print("  " + (o.strip().replace("\n", "\n  ") or "(no stdout)"))
            if e.strip():
                print("  [stderr] " + e[:400].replace("\n", "\n  "))
            all_ok = all_ok and "BATTERY_VALUE_OK" in o

        # ---- 顺序取证：证明「为什么必须断言顺序」 ----
        print("\n=== order evidence: read_dir order != sorted order ===")
        o, _ = shell.run("ls -U /sys/class/power_supply/ 2>/dev/null | tr '\\n' ' '")
        print("  ls -U (same order as read_dir) = " + o.strip())
        o, _ = shell.run("ls /sys/class/power_supply/ 2>/dev/null | tr '\\n' ' '")
        print("  ls      (sorted)                = " + o.strip())
        print("  Rust read_dir order == os.listdir; neither is sorted")
    finally:
        print("\n=== cleanup ===")
        shell.sudo("umount /sys/class/power_supply 2>/dev/null; "
                   f"rm -rf /tmp/fakebat {work_dir}; true")
        o, _ = shell.run("mount | grep -c power_supply || true")
        print("  leftover power_supply mounts = " + (o.strip() or "0"))

    print("\n" + "=" * 60)
    for n, case in enumerate(CASES, 1):
        print(f"  case{n} {case['label']}")
    print("  " + (marker if all_ok else marker.replace("_OK", "_FAILED")))
    return all_ok


def run_local_mode(so_path: str) -> int:
    """本地模式：CI 的 Linux runner 上直接跑，不需要连外部主机。"""
    if os.geteuid() != 0 and os.environ.get("SI_ALLOW_SUDO") != "1":
        print("本地模式需要 root（bind mount 权限）。"
              "CI 里请用 `sudo -n python battery_value_linux.py --local <so>`。",
              file=sys.stderr)
        return 2
    if not os.path.isfile(so_path):
        print(f"被测 .so 不存在: {so_path}", file=sys.stderr)
        return 2
    work = "/tmp/sysinf-battery-accept"
    ok = run_cases(LocalShell(), os.path.abspath(so_path), work,
                   "BATTERY_VALUE_OK")
    return 0 if ok else 1


def main() -> int:
    if len(sys.argv) >= 2 and sys.argv[1] == "--local":
        return run_local_mode(sys.argv[2] if len(sys.argv) > 2 else "")
    return run_ssh_mode()


def run_ssh_mode() -> int:
    host = sys.argv[1] if len(sys.argv) > 1 else HOST
    user = sys.argv[2] if len(sys.argv) > 2 else USER
    remote_so = sys.argv[3] if len(sys.argv) > 3 else \
        posixpath.join(REMOTE_DIR, "libsysinformer.so")
    pwd = os.environ.get("SI_SSH_PASSWORD")
    if not pwd:
        print("缺少环境变量 SI_SSH_PASSWORD（密码不落盘、不设默认值）",
              file=sys.stderr)
        return 2
    local_so = os.environ.get("SI_SO_PATH")
    if not local_so:
        print("缺少环境变量 SI_SO_PATH（指向被测 .so 的本地路径）",
              file=sys.stderr)
        return 2

    try:
        import paramiko
    except ImportError:
        print("缺少 paramiko：pip install paramiko", file=sys.stderr)
        return 2

    data = open(local_so, "rb").read()
    local_md5 = hashlib.md5(data).hexdigest()
    local_len = len(data)
    print(f"  本地: {local_len:,} bytes  md5={local_md5}")

    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    ssh.connect(host, username=user, password=pwd, timeout=20)

    def _run(cmd, timeout=180):
        _, o, e = ssh.exec_command(cmd, timeout=timeout)
        return (o.read().decode("utf-8", "replace"),
                e.read().decode("utf-8", "replace"))

    def _sudo(cmd, timeout=180):
        return _run(f"echo {shq(pwd)} | sudo -S -k sh -c {shq(cmd)}", timeout)

    shell = SshShell(_run, _sudo)

    run = _run
    sudo = _sudo

    run(f"mkdir -p {REMOTE_DIR}")

    # ---- 传输并校验 ----
    # 成功标记只在校验通过的分支内打印：守卫失效时若无条件报成功，
    # 就会执行远端 /tmp 里上一版的残留文件，表现为"装了旧版却以为装上了"。
    print("\n=== transfer ===")
    b64 = base64.b64encode(data).decode()
    chunk = 60000
    ok = False
    for attempt in range(1, 6):
        run(f"rm -f {remote_so}.b64")
        broken = False
        for i in range(0, len(b64), chunk):
            _, e = run(f"printf '%s' '{b64[i:i + chunk]}' >> {remote_so}.b64")
            if e.strip():
                print(f"  第 {attempt} 次传输 stderr: {e[:120]}")
                broken = True
                break
        if broken:
            time.sleep(3)
            continue
        o, _ = run(f"base64 -d {remote_so}.b64 > {remote_so} && "
                   f"md5sum {remote_so} | cut -d' ' -f1 && stat -c %s {remote_so}")
        lines = [x for x in o.strip().splitlines() if x]
        if len(lines) >= 2 and lines[0].strip() == local_md5 and \
                int(lines[1].strip()) == local_len:
            print(f"  TRANSFER_VERIFIED md5={local_md5} bytes={local_len:,}")
            ok = True
            break
        print(f"  TRANSFER_MISMATCH md5={lines[0] if lines else '?'}")
        time.sleep(3)
    if not ok:
        print("  传输 5 次校验均失败，中止（不将就）")
        ssh.close()
        return 1

    ok = run_cases(shell, remote_so, REMOTE_DIR,
                   "KALI_BATTERY_VALUE_OK", pwd_hint=True)
    ssh.close()
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())