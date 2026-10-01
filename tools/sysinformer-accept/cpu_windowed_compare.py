#!/usr/bin/env python3
"""CPU 窗口对齐对照 —— 用长窗口压住方差，替代瞬时配对。

为什么不用瞬时配对：
  PDH 每个样本的窗口约 1.0~2.8s（实测 Get-Counter 单次耗时 1000~2840ms），
  而本库每个样本的窗口只有 ~27ms。即便时间戳对齐到 1ms，两者的
  「被测时间区间」仍差 50 倍。机器负载在 40%~97% 间剧烈波动时，
  逐次差的标准差约 13.8pp —— 远大于待测量的几个 pp 偏差，
  于是四轮均值在 -2 ~ +4.3pp 间乱摆，无法定案。

本脚本的做法（窗口对齐）：
  PDH 每隔 W 秒给一个值（W = 该次 Get-Counter 的实际窗口）。
  本库在同一区间内连续取 N 个样本求均值。
  两侧的「被测区间」就被拉平到同一量级（W），方差随之下降。

做法细节：
  1. 后台 pwsh 流式输出 (unix_ms, _Total, 每核均值)，每行带 PDH 自报时间戳。
  2. 本进程高频采样 (unix_ms, 汇总, 每核)。
  3. 对每个 PDH 样本，取其**前一个 PDH 样本之后、本样本之前**的
     本库样本求均值（这些样本全部落在 PDH 窗口内）。
  4. 统计。
"""
import bisect
import ctypes
import json
import math
import os
import subprocess
import sys
import tempfile
import time

DLL = sys.argv[1]
DURATION = int(sys.argv[2]) if len(sys.argv) > 2 else 60

_HERE = os.path.dirname(os.path.abspath(__file__))
PS = os.path.join(_HERE, "pdh_stream.ps1")
STREAM = os.path.join(tempfile.gettempdir(), "sysinformer_pdh_stream.jsonl")
PWSH = r"C:\Program Files\PowerShell\7\pwsh.exe"

for p in (PWSH, PS):
    if not os.path.isfile(p):
        print(f"  找不到 {p}")
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


NCPU = ctypes.windll.kernel32.GetActiveProcessorCount(0xFFFF)
print(f"  逻辑核数 = {NCPU}")

if os.path.exists(STREAM):
    os.remove(STREAM)
proc = subprocess.Popen(
    [PWSH, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", PS,
     "-Out", STREAM, "-DurationSec", str(DURATION)],
    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

time.sleep(2.0)
print(f"  本库采样中（约 {DURATION - 4}s）...", flush=True)
mine = []
t_end = time.time() + DURATION - 4
while time.time() < t_end:
    ts = int(time.time() * 1000)
    d = call("system.snapshot")["data"]
    cores = [c["usage"] for c in d["cpu_cores"]]
    if cores:
        mine.append((ts, sum(cores) / len(cores)))
    time.sleep(0.02)

out, err = proc.communicate(timeout=DURATION + 90)
print(f"  PDH: {out.strip()[:100]}")

ref = []
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
        cs = [obj.get(str(i)) for i in range(NCPU)]
        cs = [c for c in cs if c is not None]
        if total is not None and cs:
            ref.append((int(ts_s), float(total), sum(cs) / len(cs)))

ref.sort()
print(f"\n  PDH 样本 = {len(ref)}   本库样本 = {len(mine)}")

if len(ref) < 4 or len(mine) < 50:
    print("  样本不足")
    sys.exit(2)

# PDH 相邻样本的时间差 = 其窗口长度
gaps = [ref[i + 1][0] - ref[i][0] for i in range(len(ref) - 1)]
print(f"  PDH 窗口（相邻样本间隔）: 中位 {sorted(gaps)[len(gaps)//2]}ms  "
      f"范围 {min(gaps)}~{max(gaps)}ms")

# ---- 窗口对齐：每个 PDH 样本配它自己窗口内的本库样本均值 ----
mine_ts = [m[0] for m in mine]
pairs, windows = [], []
for i in range(len(ref) - 1):
    ts, total, cores_avg = ref[i]
    nxt = ref[i + 1][0]
    lo = bisect.bisect_left(mine_ts, ts)
    hi = bisect.bisect_left(mine_ts, nxt)
    if hi - lo < 3:
        continue
    seg = [mine[j][1] for j in range(lo, hi)]
    avg = sum(seg) / len(seg)
    pairs.append(avg - total)
    windows.append(nxt - ts)

n = len(pairs)
mean = sum(pairs) / n
sd = math.sqrt(sum((v - mean) ** 2 for v in pairs) / (n - 1))
se = sd / math.sqrt(n)
lo_ci, hi_ci = mean - 1.96 * se, mean + 1.96 * se

print("\n" + "=" * 74)
print("窗口对齐后的对照结果（本库汇总均值 - PDH _Total）")
print("=" * 74)
print(f"  有效配对 = {n}（每对含 {3}~{max(20, int(sorted(windows)[len(windows)//2]//27))} "
      f"个本库样本，窗口中位 {sorted(windows)[len(windows)//2]}ms）")
print(f"  均值差 = {mean:+.3f}pp")
print(f"  标准差 = {sd:.2f}pp")
print(f"  标准误 = {se:.2f}pp")
print(f"  95% CI = [{lo_ci:+.3f}, {hi_ci:+.3f}]pp")
print(f"  逐次差 = {[round(v, 1) for v in pairs]}")
print("=" * 74)

EPS = 1e-6
ok = (lo_ci - EPS) <= 0 <= (hi_ci + EPS)
if ok:
    print("\n  **判定：无系统性偏差**（95% CI 含 0）")
    print("  TASKMGR_CPU_WINDOWED_OK")
    sys.exit(0)
else:
    print(f"\n  **判定：存在系统性偏差 {mean:+.2f}pp**（CI 不含 0）")
    print("  TASKMGR_CPU_WINDOWED_FAILED")
    sys.exit(1)
