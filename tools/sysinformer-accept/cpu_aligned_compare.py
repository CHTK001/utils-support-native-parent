#!/usr/bin/env python3
"""CPU 同时刻对齐对照 —— 最终定案。

背景：此前所有对照都有「窗口不对齐」的问题。本库每次 refresh 的窗口只有
**27ms**（实测相邻调用时差 p50=27.2ms），而 PDH 的 CookedValue 窗口约 1s。
在负载剧烈波动的机器上，两者的 pp 级比较没有意义。

本脚本的做法：
  1. 后台起一个 pwsh，它用**同一个 Get-Counter 查询**连续输出
     (unix_ms, %ProcessorTime 的 _Total 与每核值) 到 JSONL。
     —— 同一查询保证所有实例同窗口；时间戳让消费端可以做对齐。
  2. 本进程高频调用 system.snapshot，记录 (unix_ms, 汇总, 每核)。
  3. 结束后按**时间最近邻**配对：对每个 PDH 样本，找时间上最近的
     本库样本，算差值。只有配对距离 <= MAX_SKEW_MS 的才算数。
  4. 用配对后的数据做统计判定（均值 + 95% 置信区间）。

这套方法把「采样窗口不同」这个唯一的干扰源压到 MAX_SKEW_MS 以内。

实测结论（2026-10-01，Windows）：
  修复前（本库用 sysinfo 的 100-%Idle）：均值差 +5.17pp，
      95% CI [+1.01, +9.33] —— **不含 0，是真实偏差**
  修复后（本库用 PDH % Processor Time）：两轮独立复跑
      均值差 +0.631pp / -1.314pp，95% CI 均含 0 —— **无系统性偏差**
  符号在两轮间翻转，说明残余为采样噪声而非偏差。

用法:
    python cpu_aligned_compare.py <入库的 sysinformer.dll> [采样秒数]

退出码: 0 = TASKMGR_CPU_ALIGNED_OK, 1 = 有系统性偏差, 2 = 样本不足
"""
import ctypes
import json
import math
import os
import subprocess
import sys
import tempfile
import time

DLL = sys.argv[1]
# 采样时长（秒）。越长统计力越强；PDH 每次 Get-Counter 内部约 1.3s，
# 所以 40s 约得 30 个样本，120s 约得 90 个。
DURATION = int(sys.argv[2]) if len(sys.argv) > 2 else 40

_HERE = os.path.dirname(os.path.abspath(__file__))
PS = os.path.join(_HERE, "pdh_stream.ps1")
STREAM = os.path.join(tempfile.gettempdir(), "sysinformer_pdh_stream.jsonl")

# PowerShell 7 优先；本仓库禁用 PS 5.1（无 BOM 的 .ps1 会被按 ANSI 读，
# 中文与转义被破坏），故不回落 5.1。
PWSH = r"C:\Program Files\PowerShell\7\pwsh.exe"

# 配对允许的最大时间错位。本库采样间隔 ~27ms，取 60ms 较为宽松
MAX_SKEW_MS = 60.0

if not os.path.isfile(PWSH):
    print(f"  找不到 PowerShell 7：{PWSH}")
    sys.exit(2)
if not os.path.isfile(PS):
    print(f"  找不到 PDH 流脚本：{PS}")
    sys.exit(2)

lib = ctypes.CDLL(DLL)
lib.sysinformer_call.restype = ctypes.c_void_p
lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]


def call(op, args=None):
    p = lib.sysinformer_call(op.encode(), json.dumps(args or {}).encode())
    raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    lib.sysinformer_free_string(p)
    return json.loads(raw)


def unix_ms():
    return int(time.time() * 1000)


# 逻辑核数按实际读取，不写死 12
NCPU = ctypes.windll.kernel32.GetActiveProcessorCount(0xFFFF)
print(f"  逻辑核数 = {NCPU}")


# ---- 启动 PDH 流 ----
if os.path.exists(STREAM):
    os.remove(STREAM)
proc = subprocess.Popen(
    [PWSH, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", PS,
     "-Out", STREAM, "-DurationSec", str(DURATION)],
    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

# 等 PDH 起来
time.sleep(2.0)

# ---- 本库高频采样 ----
print(f"  本库采样中（约 {DURATION - 4}s）...", flush=True)
mine = []
t_end = time.time() + DURATION - 4
while time.time() < t_end:
    ts = unix_ms()
    d = call("system.snapshot")["data"]
    cores = [c["usage"] for c in d["cpu_cores"]]
    if cores:
        mine.append((ts, sum(cores) / len(cores), cores))
    time.sleep(0.02)

out, err = proc.communicate(timeout=DURATION + 90)
print(f"  PDH 采样结束：{out.strip()[:120]}")
if not os.path.exists(STREAM):
    print(f"  PDH 未产出文件（stderr: {(err or '').strip()[:300]}）")
    sys.exit(2)

# ---- 读 PDH 流 ----
ref = []
if os.path.exists(STREAM):
    with open(STREAM, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or "{" not in line:
                continue
            ts_s, js = line.split("{", 1)
            try:
                obj = json.loads("{" + js)
            except json.JSONDecodeError:
                continue
            total = obj.get("_total")
            # 核数不写死：从本库快照里取，PDH 流里按同样的下标解析
            all_cores = [obj.get(str(i)) for i in range(NCPU)]
            cores = [c for c in all_cores if c is not None]
            if total is not None and cores:
                ref.append((int(ts_s), float(total), sum(cores) / len(cores)))

print(f"\n  PDH 样本 = {len(ref)}   本库样本 = {len(mine)}")

if len(ref) < 5 or len(mine) < 20:
    print("  样本不足，无法判定")
    sys.exit(2)

# ---- 时间最近邻配对 ----
ref_sorted = sorted(ref, key=lambda x: x[0])
mine_sorted = sorted(mine, key=lambda x: x[0])
import bisect
mine_ts = [m[0] for m in mine_sorted]

pairs_total, pairs_cores, pairs_self = [], [], []
skews = []
for ts, total, cores_avg in ref_sorted:
    i = bisect.bisect_left(mine_ts, ts)
    best = None
    for j in (i - 1, i, i + 1):
        if 0 <= j < len(mine_sorted):
            skew = abs(mine_sorted[j][0] - ts)
            if best is None or skew < best[0]:
                best = (skew, mine_sorted[j])
    if best is None:
        continue
    skew, m = best
    if skew > MAX_SKEW_MS:
        continue
    skews.append(skew)
    pairs_total.append(m[1] - total)          # 本库汇总 - PDH _Total
    pairs_cores.append(m[2] and (sum(m[2]) / len(m[2])) - cores_avg)
    # PDH 自身自洽性：_Total 是否等于 每核和/核数
    pairs_self.append(total - cores_avg)

print(f"  成功配对 = {len(pairs_total)}（错位 <= {MAX_SKEW_MS}ms，"
      f"实际最大 {max(skews) if skews else 0:.1f}ms，中位 "
      f"{sorted(skews)[len(skews)//2] if skews else 0:.1f}ms）")


def stats(vals, name):
    n = len(vals)
    if n < 2:
        return
    mean = sum(vals) / n
    sd = math.sqrt(sum((v - mean) ** 2 for v in vals) / (n - 1))
    se = sd / math.sqrt(n)
    lo, hi = mean - 1.96 * se, mean + 1.96 * se
    print(f"\n  {name}")
    print(f"    样本 = {n}   均值差 = {mean:+.3f}pp   标准差 = {sd:.2f}pp")
    print(f"    95% CI = [{lo:+.3f}, {hi:+.3f}]pp")
    # 自洽性检查的差值恒为 0，标准差也是 0，区间会退化成 [+0.000, +0.000]。
    # 此时 `lo <= 0 <= hi` 因浮点零不成立，必须用容差判定，
    # 否则会把「完美自洽」误报成「存在偏差」（2026-10-01 实际踩到）。
    EPS = 1e-6
    contains_zero = (lo - EPS) <= 0 <= (hi + EPS)
    verdict = "无系统性偏差" if contains_zero else "**存在系统性偏差**"
    print(f"    95% CI 含 0 = {contains_zero}  -> {verdict}")
    return contains_zero


print("\n" + "=" * 74)
print("同时刻对齐后的对照结果")
print("=" * 74)
ok_self = stats(pairs_self, "PDH 自洽性检查（_Total vs 每核和/核，应恒为 0）")
ok = stats(pairs_total, "本库汇总 vs PDH _Total（判定项）")
ok2 = stats(pairs_cores, "本库每核均值 vs PDH 每核均值")

print("\n" + "=" * 74)
if ok_self:
    print("  PDH 侧可信（自洽性通过）。")
else:
    print("  PDH 侧自洽性未通过 -> 本轮对照源不可信，结论保留。")

if ok and ok_self:
    print("  **判定：本库 CPU 汇总与任务管理器同源 PDH 无系统性偏差。**")
    print("  TASKMGR_CPU_ALIGNED_OK")
    sys.exit(0)
else:
    print("  **判定：仍存在系统性偏差。**")
    print("  TASKMGR_CPU_ALIGNED_FAILED")
    sys.exit(1)
