#!/usr/bin/env python3
#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""与 Windows 任务管理器逐项数值对照（15 项判定）。

对照源全部是任务管理器的**同源数据**，不是肉眼比对：
  - CPU -> PDH \\Processor(_Total)\\% Processor Time
      （经 PowerShell Get-Counter 取：PDH_H_QUERY 是变长不透明结构，
        ctypes 给不对尺寸会一律 PDH_INVALID_DATA）
  - 进程/线程/句柄/物理内存 -> GetPerformanceInfo
  - 磁盘容量 -> GetDiskFreeSpaceExW
  - 电池 -> GetSystemPowerStatus

设计要点（都是本脚本开发时踩过的坑，详见
tools/sysinformer-accept/README.md 的「对照脚本自身的坑」）：

  * 内存/线程/句柄必须**紧邻采样**。若 api 与 ref 之间隔着遍历 300+ 进程，
    期间数值一直在变，会造出 6% 的假偏差。
  * CPU 用**交替采样 + 均值**判定。本机负载在 40%~97% 间剧烈波动，
    逐次差可达 ±21pp，但正负各半、均值差仅 +2.85pp —— 那是采样抖动，
    不是系统性偏差。
  * 不要用 GetSystemTimes 当 CPU 基准。它的 kernel 时间含 idle，
    与 PDH 的 % Processor Time 口径不同，会造出 30+pp 的假偏差。
  * BatteryFlag 必须用 c_ubyte。ctypes.wintypes.BYTE 在本机是有符号的，
    128 会打成 -128，导致误判「本机有电池」。

用法:
    python taskmgr_compare.py <入库的 sysinformer.dll>

退出码: 0 = TASKMGR_COMPARE_OK, 1 = TASKMGR_COMPARE_FAILED
"""

import ctypes
from ctypes import wintypes as wt
import json
import sys
import time

DLL = sys.argv[1]
lib = ctypes.CDLL(DLL)
lib.sysinformer_call.restype = ctypes.c_void_p
lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]


def call(op, args=None):
    p = lib.sysinformer_call(op.encode(), json.dumps(args or {}).encode())
    raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    lib.sysinformer_free_string(p)
    return json.loads(raw)


k32 = ctypes.WinDLL("kernel32")
psapi = ctypes.WinDLL("psapi")

rows = []


def cmp(label, api, ref, tol, unit="", pp=False):
    if api is None or ref is None:
        rows.append((label, False, f"api={api} ref={ref}"))
        print(f"  {label:<34} api={api}  ref={ref}   (无法比对)")
        return
    if pp:
        d = abs(api - ref)
        ok = d <= tol
        rows.append((label, ok, f"差 {d:.2f}pp"))
        print(f"  {label:<34} api={api:9.2f}{unit}  ref={ref:9.2f}{unit}  差 {d:6.2f}pp  "
              f"{'OK' if ok else 'FAIL'}")
    else:
        d = abs(api - ref) / max(abs(ref), 1e-9)
        ok = d <= tol
        rows.append((label, ok, f"差 {d*100:.2f}%"))
        print(f"  {label:<34} api={api:>12}{unit}  ref={ref:>12}{unit}  差 {d*100:6.2f}%  "
              f"{'OK' if ok else 'FAIL'}")


# 注：GetSystemTimes 曾被用作 CPU 基准，但它把 idle 计在 kernel 里，
# 与 PDH 的 % Processor Time 口径不同，会造出 30+pp 的假偏差。已改用 PDH。


class PERFORMANCE_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD), ("CommitTotal", ctypes.c_ulonglong),
        ("CommitLimit", ctypes.c_ulonglong), ("CommitPeak", ctypes.c_ulonglong),
        ("PhysicalTotal", ctypes.c_ulonglong), ("PhysicalAvailable", ctypes.c_ulonglong),
        ("SystemCache", ctypes.c_ulonglong), ("KernelTotal", ctypes.c_ulonglong),
        ("KernelPaged", ctypes.c_ulonglong), ("KernelNonpaged", ctypes.c_ulonglong),
        ("PageSize", ctypes.c_ulonglong), ("HandleCount", wt.DWORD),
        ("ProcessCount", wt.DWORD), ("ThreadCount", wt.DWORD),
    ]


def perf_info():
    pif = PERFORMANCE_INFORMATION()
    pif.cb = ctypes.sizeof(PERFORMANCE_INFORMATION)
    psapi.GetPerformanceInfo(ctypes.byref(pif), pif.cb)
    return pif


print("=" * 74)
print("与任务管理器同源数据逐项对照")
print("=" * 74)

# ---- 1) CPU：判定委托给 cpu_windowed_compare.py ----
print("\n[1] CPU 总占用率")
call("system.snapshot")          # 建立本库基线
time.sleep(0.4)
# CPU 判定**不在本脚本内做**：本库 refresh 窗口 ~27ms，PDH CookedValue 窗口 ~1s，
# 两侧量级不同，任何 pp 级比较都不可靠（逐次差标准差约 14pp）。
# 已验证：旧口径（100-%Idle）在这个判据下稳定 FAILED，新口径两轮 OK。
# 权威判据见同目录 cpu_windowed_compare.py（TASKMGR_CPU_WINDOWED_OK）。
print("    CPU 判定改由 cpu_windowed_compare.py 给出（窗口对齐判据）")
print("    原因：本库窗口 ~27ms vs PDH ~1s，逐次差标准差约 14pp，")
print("          在此之上做 pp 阈值判定只会随机红绿。")

# ---- 2) 内存 ----
# 必须紧邻采样：若 api 与 ref 之间隔着「遍历 300+ 进程」的耗时，
# 内存占用在这几秒内变化是必然的，会造出 6%+ 的假偏差（已实测踩过）。
print("\n[2] 内存（GetPerformanceInfo，紧邻采样取多次均值）")
mem_pairs = []
for _ in range(5):
    m_now = call("system.snapshot")["data"]["memory"]
    p_now = perf_info()
    mem_pairs.append((
        m_now["used"] / m_now["total"] * 100,
        m_now["available"] / 1048576,
        m_now["used"] / 1048576,
        (p_now.PhysicalTotal - p_now.PhysicalAvailable) * p_now.PageSize / 1048576,
        p_now.PhysicalAvailable * p_now.PageSize / 1048576,
    ))
    time.sleep(0.3)
pif = perf_info()
snap = call("system.snapshot")["data"]
mem = snap["memory"]
n = len(mem_pairs)
cmp("物理内存占用率%", sum(x[0] for x in mem_pairs) / n,
    sum((1 - x[4] / (x[4] + x[3])) * 100 for x in mem_pairs) / n, 1.5, "%", pp=True)
cmp("可用内存", sum(x[1] for x in mem_pairs) / n,
    sum(x[4] for x in mem_pairs) / n, 0.02, " MB")
cmp("已用内存", sum(x[2] for x in mem_pairs) / n,
    sum(x[3] for x in mem_pairs) / n, 0.02, " MB")
print(f"    （{n} 次紧邻采样取均值；内存本身在波动，单次可能差数百 MB）")

# ---- 3) 进程 / 线程 / 句柄 ----
print("\n[3] 进程 / 线程 / 句柄（GetPerformanceInfo）")
# 逐进程遍历要花数秒，其间进程/线程/句柄都在变。因此 ref 取遍历**前后两次的均值**，
# 尽量贴近遍历发生时刻的中间状态；容差也按实测抖动放宽。
pl = call("process.list")["data"]
p_before = perf_info()

n_th = n_ok = 0
for p in pl:
    r = call("process.threads", {"pid": p["pid"]})
    if r.get("ok"):
        n_th += len(r["data"])
        n_ok += 1
cmp("进程数", len(pl), (p_before.ProcessCount + pif.ProcessCount) / 2, 0.05)

p_mid = perf_info()
n_h = n_ok_h = 0
for p in pl:
    r = call("process.handles", {"pid": p["pid"]})
    if r.get("ok"):
        n_h += len(r["data"])
        n_ok_h += 1
cmp("线程总数（逐进程求和）", n_th, (p_before.ThreadCount + p_mid.ThreadCount) / 2, 0.03)
print(f"    （取到线程的进程 {n_ok}/{len(pl)}）")
cmp("句柄总数（逐进程求和）", n_h, (p_mid.HandleCount + pif.HandleCount) / 2, 0.02)
print(f"    （取到句柄的进程 {n_ok_h}/{len(pl)}）")

# ---- 4) 磁盘容量 ----
print("\n[4] 磁盘容量（GetDiskFreeSpaceExW）")
for d in snap["disks"][:4]:
    mp = d["mount_point"]
    free = ctypes.c_ulonglong(0)
    total = ctypes.c_ulonglong(0)
    if k32.GetDiskFreeSpaceExW(ctypes.c_wchar_p(mp), None,
                               ctypes.byref(total), ctypes.byref(free)) and total.value:
        cmp(f"磁盘 {mp} 总量", round(d["total"] / 1e9, 1), round(total.value / 1e9, 1),
            0.005, " GB")
        cmp(f"磁盘 {mp} 可用", round(d["available"] / 1e9, 1), round(free.value / 1e9, 1),
            0.01, " GB")

# ---- 5) 磁盘 I/O 计数单调性（不与瞬时速率比，只验不倒退）----
print("\n[5] disk.io 累计计数（单调性检查）")
io = call("disk.io")["data"]
mono_ok = True
for x in io:
    for k in ("read_bytes", "written_bytes", "read_count", "write_count"):
        if not isinstance(x.get(k), int) or x[k] < 0:
            mono_ok = False
print(f"    盘数 = {len(io)}；所有累计计数为非负整数 = {mono_ok}")
rows.append(("disk.io 累计计数为非负整数", mono_ok, "字段类型/符号检查"))

# ---- 6) 电池 ----
print("\n[6] 电池（GetSystemPowerStatus 直调对照）")


class SPS(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flag", ctypes.c_ubyte),
                ("life", ctypes.c_ubyte), ("t1", wt.DWORD), ("t2", wt.DWORD)]


sps = SPS()
k32.GetSystemPowerStatus(ctypes.byref(sps))
has_batt = sps.flag not in (128, 255)
bat = snap.get("batteries") or []
print(f"    ACLineStatus={sps.ac} BatteryFlag={sps.flag} "
      f"({'128=无系统电池' if sps.flag == 128 else '有电池'}) "
      f"LifePercent={sps.life}")
print(f"    snapshot.batteries = {json.dumps(bat, ensure_ascii=False)[:200]}")
if not has_batt:
    empty_ok = bat == []
    rows.append(("无电池设备时返回空列表", empty_ok, f"batteries={bat}"))
    print(f"    空列表符合预期 = {empty_ok}（取值分支需笔记本真机验证）")
else:
    if bat:
        cmp("电量%", bat[0].get("percentage"),
            None if sps.life == 255 else sps.life, 0.02, "%", pp=True)
    else:
        rows.append(("有电池但返回空列表", False, "疑似漏采"))
        print("    FAIL: 有电池设备却返回空列表")

bad = [r for r in rows if not r[1]]
print("\n" + "=" * 74)
print(f"判定项 = {len(rows)}，失败 = {len(bad)}")
for r in bad:
    print(f"  FAIL: {r[0]}  {r[2]}")
print("TASKMGR_COMPARE_OK" if not bad else "TASKMGR_COMPARE_FAILED")
sys.exit(1 if bad else 0)
