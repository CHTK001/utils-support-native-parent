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
  1. 后台 pwsh 流式输出 (unix_ms, _Total)，每行带 PDH 自报时间戳。
     **时间戳必须用计数器自报的 $s.Timestamp**：Get-Counter 内部耗时
     1000~2840ms（实测），CookedValue 描述的是调用**结束时**那段窗口，
     用自己的墙钟打戳会整体偏移 1~2.8s。
  2. 本进程高频采样 (unix_ms, cpu.usage)。
  3. 对每个 PDH 样本，取其**前一个 PDH 样本之后、本样本之前**的
     本库样本求均值（这些样本全部落在 PDH 窗口内）。
  4. 统计，用 95% 置信区间是否含 0 作判定。

参考源为什么是 cpu.usage（= PDH _Total）而不是「每核均值」：
  2026-10-01 在 CI 的 4 核 runner 上，用「每核均值」作参考报出
  +18.965pp（标准差仅 2.04pp，很稳定），而同一判据在本机 12 核上 <2pp。
  本机进一步实测排除了库的问题：
    - PDH 的 _Total 与「全部实例均值」精确相等（差 -0.000）
    - 本库 cpu.usage 与「每核均值」精确相等（差 0.000pp，无口径混用）
  即「每核均值」只在「PDH 实例数 == 本库核数」时才等于 _Total。
  任务管理器顶部那个数字就是 _Total 本身，用它才是真正同源、
  且不依赖核数假设。

  本脚本仍打印「库内 cpu.usage 与每核均值的最大差」作诊断：该值应恒为 0；
  不为 0 说明某些核回退到 sysinfo 口径（真缺陷）。
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


def sample_once():
    """取一次快照，返回 (cpu.usage 即 PDH _Total, 每核均值, 每核列表)。"""
    d = call("system.snapshot")["data"]
    cores = [c["usage"] for c in d["cpu_cores"]]
    if not cores:
        return None, None, []
    cpu = d.get("cpu") or {}
    total = cpu.get("usage")
    if total is None:
        total = sum(cores) / len(cores)
    return float(total), sum(cores) / len(cores), cores


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
mix_diffs = []
t_end = time.time() + DURATION - 4
while time.time() < t_end:
    ts = int(time.time() * 1000)
    total, cores_avg, cores = sample_once()
    if cores:
        mine.append((ts, total))
        # 诊断：库内部汇总与每核均值是否同源。差应恒为 0；
        # 不为 0 说明某些核回退到了 sysinfo 口径（真缺陷，需单独立项）。
        mix_diffs.append(abs(total - cores_avg))
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
        # _Total 是权威参考：任务管理器顶部那个数字就是它。
        # 不再用「每核均值」当参考 —— 那要求 PDH 实例数与本库核数严格相等，
        # 而 CI 的 4 核 runner 上实测会差 19pp（见文件头说明）。
        if total is not None:
            ref.append((int(ts_s), float(total)))

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
    ts, total = ref[i]
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
print("窗口对齐后的对照结果（本库 cpu.usage[=PDH _Total] - PDH _Total）")
print("=" * 74)
if mix_diffs:
    mx = max(mix_diffs)
    # 只报告事实，不在这里断言原因。
    # 实测（400 次 / 间隔 5ms）：85% 完全一致，其余为单边负偏差、<=6pp，
    # 属 PDH 极短采样窗口的量化现象；口径混用会表现为**稳定正**偏差
    # （100-%Idle 比 %ProcessorTime 高 3~5pp，CI 不含 0），与此不同。
    # 判据本身用 cpu.usage 作参考，因此这个诊断不影响判定结果。
    print(f"  [诊断] 库内 cpu.usage 与每核均值的最大差 = {mx:.4f}pp"
          f"（超 0.01pp 的采样占比 "
          f"{sum(1 for x in mix_diffs if x > 0.01) / len(mix_diffs) * 100:.0f}%）")
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
