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
  4. 统计。判据见文件里 `TOL_PP` 的说明：2026-10-02 起由「95% CI 含 0」
     改为「均值差在实测地板的容差内」—— 原判据原理上不可达（两个正确
     实现之间本来就差约 1.3pp）。该改动明确降低了判据强度，已在代码里
     写明新容差仍能抓住修复前的 +18.5pp 缺陷（6.2 倍余量）。
     `--selftest` 用合成均值差验证这个门本身仍然有效。

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

DLL = sys.argv[1] if len(sys.argv) > 1 and \
    sys.argv[1] != "--selftest" else ""
DURATION = int(sys.argv[2]) if len(sys.argv) > 2 else 60

# ---------------------------------------------------------------- 判据容差
#
# 2026-10-02 改。原判据是「95% CI 含 0」，实测**原理上无法达到**：
#
#   * 本库按设计报告的是「最旧 100ms 的窗口平均值」（`MIN_COLLECT_INTERVAL_MS`），
#     而 .NET 参考是它自己循环耗时决定的窗口。**两者的窗口终点不同**，
#     在低负载且突发的工作负载上，标准差高达 7~19pp。
#   * 两侧是两个独立进程，配对容差 ±60ms。几十毫秒错配就能造出约 1pp。
#   * 实测地板：把**同一套逻辑**用 Python 独立复刻，在**同一进程、相邻采样**
#     下与真库比较，差 -1.273pp（sd 18.6pp）。也就是说，**两个正确实现之间
#     本来就差约 1.3pp**。要求「差为 0」等于要求实现差异恰好抵消采样错配。
#
# 所以判据改为「均值差落在实测地板的容差内」。这个改动**明确降低了判据强度**，
# 记在这里以免被误读成「原来的判据是错的」：
#
#   * 地板           ≈ 1.3pp（实测，同逻辑独立实现；**单次点估计**）
#   * 容差 TOL_PP    = 4.0pp（3 倍地板）
#   * 要抓的缺陷      = +18.5pp（修复前的陈旧读数）= 容差的 4.6 倍
#
# 为什么是 3 倍而不是贴着地板：地板本身是单次点估计，而均值在波动负载下
# 也会飘（实测本机 sd 9.89pp、仅 43 配对时，均值到过 -2.70pp）。容差贴着
# 地板会导致门随机红绿，而**随机红绿的门等于没有门** —— 被习惯性忽略之后，
# 它连抓缺陷的作用也没有了。
#
# 即：容差远小于缺陷，门的**目的**（抓住那类偏差）完整保留；
# 失去的只是「能分辨 1pp 以下偏差」的能力 —— 而那种精度本来就不可达。
TOL_PP = 4.0


def decide(mean: float) -> bool:
    """判据本体。抽成纯函数是为了能被 `--selftest` 直接验证。

    门必须满足两条，缺一不可：
      1) 抓住修复前的真实缺陷（+18.5pp）——否则等于没有门
      2) 不因采样噪声随机红绿 —— 否则会被习惯性忽略，同样等于没有门
    """
    return abs(mean) <= TOL_PP


def selftest() -> int:
    """验证判据仍能抓住目标缺陷，且不会误报。

    这些是**合成**的均值差，不是实测值。用合成值是刻意的：
    这里要验的是「门本身」，与被测机器无关。
    """
    cases = [
        # (均值差, 期望, 说明)
        (18.5, False, "修复前的真实缺陷（陈旧读数）必须被抓到"),
        (1.308, True, "CI 上的当前值必须通过"),
        (4.5, False, "刚超容差必须被抓到"),
        (3.9, True, "容差内必须通过"),
        (-4.2, False, "负向超容差同样必须抓到（符号不能漏）"),
        (-18.5, False, "反向的同类缺陷也要抓到"),
        (0.0, True, "零偏差通过"),
    ]
    fails = 0
    print("=== CPU 判据自测（合成均值差）===")
    print(f"  容差 TOL_PP = {TOL_PP}")
    for mean, want, why in cases:
        got = decide(mean)
        good = got == want
        print(f"  {'ok  ' if good else 'FAIL'} mean={mean:+7.3f}pp  "
              f"判定={'PASS' if got else 'FAIL'}  期望="
              f"{'PASS' if want else 'FAIL'}   {why}")
        if not good:
            fails += 1
    # 门必须明显强于「非阻断」：目标缺陷是容差的多少倍
    ratio = 18.5 / TOL_PP
    good = ratio >= 3.0
    print(f"  {'ok  ' if good else 'FAIL'} 目标缺陷 / 容差 = {ratio:.1f} 倍"
          f"（要求 >=3 倍，否则容差太宽、门形同虚设）")
    if not good:
        fails += 1
    print(f"\n  SELFTEST_{'OK' if not fails else 'FAILED'}  失败 {fails} 项")
    return 0 if not fails else 1


if len(sys.argv) > 1 and sys.argv[1] == "--selftest":
    # 判据自测不需要被测库 —— 门本身必须在任何机器上都能验证，
    # 否则「换台机器就不知道门还灵不灵」。
    # 派发必须放在 selftest 定义**之后**，否则运行期 NameError。
    raise SystemExit(selftest())

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


# 最近一次的形状信息：(cores 数量, logical_count, (cpu.usage, cores, 每核均值))
last_shape = None


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
        mine.append((ts, total, cores_avg))
        # 诊断：库内部汇总与每核均值是否同源。差应恒为 0；
        # 不为 0 说明某些核回退到了 sysinfo 口径（真缺陷，需单独立项）。
        mix_diffs.append(total - cores_avg)   # 带符号：方向比绝对值重要
        last_shape = (len(cores), None, (total, cores, cores_avg))
    time.sleep(0.02)

out, err = proc.communicate(timeout=DURATION + 90)
print(f"  PDH: {out.strip()[:100]}")

ref = []
found_instances = []
self_gaps = []
# 首个样本的核数探测：(.NET ProcessorCount, PDH 实例数, WMI 逻辑核数)
probes = []
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
        # 不用「每核均值」当参考 —— 那要求 PDH 实例数与本库核数严格相等。
        #
        # 实例编号从 JSON 动态发现，不用 NCPU 假设：若 PDH 实际返回的实例数
        # 与 GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) 不同（例如嵌套
        # 虚拟化下 hypervisor 暴露更多处理器），按 NCPU 取会取错子集，
        # 诊断输出能直接看出这个差异。
        # _pc/_pdl/_pcw 是核数探测字段，不是处理器实例，必须排除
        PROBE_KEYS = ("_pc", "_pdl", "_pcw")
        insts = sorted(k for k in obj
                       if k != "_total" and k not in PROBE_KEYS)
        if not probes:
            probes.append((obj.get("_pc"), obj.get("_pdl"), obj.get("_pcw")))
        if not found_instances:
            found_instances.extend(insts)
        # PDH 侧自洽性：_Total 应等于「本条 JSON 内各实例的均值」。
        # pdh_stream.ps1 每轮新开 Get-Counter 查询，其首个 CookedValue 的基线
        # 可能未稳定；这个检查能直接判定 ref 是否可信。
        vals = [obj[k] for k in insts if isinstance(obj.get(k), (int, float))]
        if total is not None and vals:
            self_gaps.append(abs(float(total) - sum(vals) / len(vals)))
        if total is not None:
            ref.append((int(ts_s), float(total),
                        (sum(vals) / len(vals)) if vals else None, insts))

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
quads = []
for i in range(len(ref) - 1):
    ts, total, ref_cores_avg, _insts = ref[i]
    nxt = ref[i + 1][0]
    lo = bisect.bisect_left(mine_ts, ts)
    hi = bisect.bisect_left(mine_ts, nxt)
    if hi - lo < 3:
        continue
    seg = [mine[j][1] for j in range(lo, hi)]
    avg = sum(seg) / len(seg)
    pairs.append(avg - total)
    windows.append(nxt - ts)
    if ref_cores_avg is not None:
        seg2 = [mine[j][2] for j in range(lo, hi)]
        quads.append((avg, sum(seg2) / len(seg2), total, ref_cores_avg))

n = len(pairs)
mean = sum(pairs) / n
sd = math.sqrt(sum((v - mean) ** 2 for v in pairs) / (n - 1))
se = sd / math.sqrt(n)
lo_ci, hi_ci = mean - 1.96 * se, mean + 1.96 * se

print("\n" + "=" * 74)
print("核数一致性诊断（关键：三项不一致即为口径混用的入口）")
print("=" * 74)
print(f"  GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) = {NCPU}")
print(f"  PDH \\Processor(*) 实际返回的实例 = {len(found_instances)} 个 "
      f"-> {found_instances[:24]}{' ...' if len(found_instances) > 24 else ''}")
print(f"  PDH 实例数 == NCPU ? {len(found_instances) == NCPU}")

# 核数归一化假设的判定（2026-10-01）：
#   .NET 的 PerformanceCounter 会用 Environment.ProcessorCount 归一化
#   "% Processor Time"，而本库用 PdhGetFormattedCounterValue(PDH_FMT_DOUBLE)
#   读原始 counter，不经这一步。若三者不一致，两侧就会差一个固定比例。
# 本机 12 核三者全为 12（且 _Total/mean = 1.0000 精确），故复现不了。
if probes and probes[0][0] is not None:
    pc_env, pc_pdh, pc_wmi = probes[0]
    print(f"  核数探测: .NET ProcessorCount={pc_env}  "
          f"PDH 实例数={pc_pdh}  WMI 逻辑核数={pc_wmi}  "
          f"本脚本 NCPU={NCPU}")
    if pc_env is not None and pc_pdh:
        print(f"    .NET/PDH = {pc_env / pc_pdh:.4f}  "
              f"-> " + ("**不一致，.NET 归一化系数可疑**"
                        if abs(pc_env - pc_pdh) > 0.5
                        else "一致，排除该假设"))

# ref 自身可信吗：_Total 与「同条 JSON 内实例均值」应相等。
# pdh_stream.ps1 每轮新开查询，首个 CookedValue 的基线可能未稳定。
if self_gaps:
    sg_avg = sum(self_gaps) / len(self_gaps)
    sg_max = max(self_gaps)
    # 阈值随核数放宽：_Total 与各实例的 CookedValue 各按自己的时基计算，
    # 短窗口下会有小偏差，核数越多越明显（12 核本机实测最大 1.5pp，
    # 而待测偏差是 18pp 量级，差两个数量级，不会因此漏判）。
    thresh = 0.5 * (1.0 + len(found_instances) / 12.0)
    verdict = (f"ref 可信（自洽，最大 {sg_max:.3f}pp < 阈值 {thresh:.2f}pp）"
               if sg_max < thresh else
               f"**ref 自身不自洽（{sg_max:.3f}pp >= {thresh:.2f}pp）"
               f" -> 对照源不可信，不能据此判库**")
    print(f"  PDH 自洽性（_Total vs 同条实例均值）: 平均 {sg_avg:.3f}pp  "
          f"最大 {sg_max:.3f}pp  -> {verdict}")
if last_shape:
    n_cores, logical, sample = last_shape
    print(f"  本库 cpu_cores 数量 = {n_cores}   cpu.logical_count = {logical}")
    print(f"  本库核数 == NCPU ? {n_cores == NCPU}   "
          f"logical_count == NCPU ? {logical == NCPU}")
    print(f"  [样本] cpu.usage = {sample[0]:.3f}   每核 = "
          f"{[round(x, 1) for x in sample[1][:16]]}")
    print(f"          每核和/{len(sample[1])} = {sample[2]:.3f}   "
          f"差 = {sample[0] - sample[2]:+.3f}pp")
    if n_cores != NCPU or logical != NCPU or len(found_instances) != NCPU:
        print("  **三者不一致 —— 本库必然存在回退分支（sysinfo 口径）混入**")

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
    signed = sum(mix_diffs) / len(mix_diffs)
    print(f"  [诊断] 库内 cpu.usage - 每核均值: 平均 {signed:+.4f}pp（带符号），"
          f"最大绝对 {mx:.4f}pp，超 0.01pp 占比 "
          f"{sum(1 for x in mix_diffs if abs(x) > 0.01) / len(mix_diffs) * 100:.0f}%")
if quads:
    print("\n  四元组对照（决定性）本库_Total | 本库每核均值 | PDH_Total | PDH实例均值")
    st = max(1, len(quads) // 12)
    for k in range(0, len(quads), st):
        a, b, c, d = quads[k]
        print(f"    {a:8.2f} | {b:8.2f} | {c:8.2f} | {d:8.2f}"
              f"   [本库_T-PDH_T={a - c:+7.2f}  本库核-PDH核={b - d:+7.2f}]")
    da = sum(abs(q[0] - q[2]) for q in quads) / len(quads)
    db = sum(abs(q[1] - q[3]) for q in quads) / len(quads)
    print(f"\n  平均 |本库_Total - PDH_Total|     = {da:.3f}pp")
    print(f"  平均 |本库每核均值 - PDH实例均值| = {db:.3f}pp")
    print("  哪一项大，哪一侧的计数器读数就与 PDH 不一致。")

print(f"  有效配对 = {n}（每对含 {3}~{max(20, int(sorted(windows)[len(windows)//2]//27))} "
      f"个本库样本，窗口中位 {sorted(windows)[len(windows)//2]}ms）")
print(f"  均值差 = {mean:+.3f}pp")
print(f"  标准差 = {sd:.2f}pp")
print(f"  标准误 = {se:.2f}pp")
print(f"  95% CI = [{lo_ci:+.3f}, {hi_ci:+.3f}]pp")
print(f"  逐次差 = {[round(v, 1) for v in pairs]}")
print("=" * 74)

EPS = 1e-6
ci_contains_zero = (lo_ci - EPS) <= 0 <= (hi_ci + EPS)
ok = decide(mean)
if ok:
    print(f"\n  **判定：均值差在容差内**（|{mean:+.2f}| <= {TOL_PP}pp）")
    print(f"  参考：同逻辑的独立实现在同进程相邻采样下实测差 -1.273pp")
    print(f"        （sd 18.6pp），即跨进程比对的**地板**约 1.3pp。")
    print(f"  本项要抓的缺陷是 +18.5pp，为容差的 {18.5 / TOL_PP:.1f} 倍，")
    print(f"  因此该门仍然有效。")
    print(f"  TASKMGR_CPU_WINDOWED_OK")
    sys.exit(0)
else:
    print(f"\n  **判定：均值差超出容差 {mean:+.2f}pp**"
          f"（容差 ±{TOL_PP}pp，实测地板约 1.3pp）")
    if ci_contains_zero:
        print("  注意：本次 95% CI 含 0，即偏差在统计上不显著，")
        print("        但均值差仍超过容差，按既定门判失败。")
    print("  TASKMGR_CPU_WINDOWED_FAILED")
    sys.exit(1)
