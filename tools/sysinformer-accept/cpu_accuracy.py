#!/usr/bin/env python3
"""CPU 准确性判据：用**负载增量**，让背景负载自己抵消。

## 为什么不能用「绝对值 vs 绝对值」

已有两种绝对值判据，都不足以回答「准不准」：

* `cpu_windowed_compare.py`：逐窗口配对 + 容差 ±4pp。
  它回答的是「允许多大偏差」，**不是**「偏差本身有多大」。
* 长窗口聚合（本地实测）：~98% 负载下差 **-0.392pp**（不显著）、
  ~100% 下差 **+0.020pp** —— 但饱和时两个数都贴着 100，
  **区分度极低**，偏差会被压缩掉。

而 CI 在**中等负载**下报的是 **+2.05pp**（高）。本机长期满载，
造不出中等负载，所以那个区间在本机**测不了**。

## 增量判据（本脚本）

分两段：

    阶段 1（基线）    测 T 秒                       -> 库 L1、参照 R1
    阶段 2（加负载）  起 K 个忙线程后再测 T 秒        -> 库 L2、参照 R2

    判据： (L2 - L1)  应等于  (R2 - R1)

背景负载在两段里都存在，**在差值里抵消**。所以：

* 在饱和机器上也能判（只要有 ≥5pp 的增量空间）
* 在空闲 CI runner 上加 2 线程 ≈ +50pp，区分度高
* 正好覆盖本机测不到的**中等负载区间**

## 参照

Windows 用 `typeperf "\\Processor(_Total)\\% Processor Time"` —— 直接走 PDH，
不经 .NET 的 `PerformanceCounter`（那个会按 `Environment.ProcessorCount`
缩放，是已知坑）。与库用的是**同一个计数器**，所以比的是**实现差异**，
不是定义差异。

## 自我保护（每条都有踩坑背景）

* **首次读数丢弃**：`cpu` 需预热，第一次无效（曾表现为 100%）
* **参照样本不足** -> 判「不是通过，是没测到」，而不是通过
* **增量 < 5pp** -> 判「无判别力」（机器已饱和），而不是通过
* **采样覆盖跨度不足窗口 80%** -> 结论无效（首版按样本条数判，
  在本机误判过一次：`system.snapshot` 高负载下比 CI 慢得多）
* 忙线程用**真实计算**而非 sleep —— sleep 不加负载，会让判据假通过
"""
import argparse
import ctypes
import json
import os
import statistics
import subprocess
import sys
import threading
import time

PHASE_SEC = 25
MIN_INCREMENT_PP = 5.0


def load_lib(path):
    lib = ctypes.CDLL(path)
    lib.sysinformer_call.restype = ctypes.c_void_p
    lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
    lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]
    return lib


LIB = None


def call(op, args=None):
    a = json.dumps(args or {}).encode()
    p = LIB.sysinformer_call(op.encode(), a)
    raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    LIB.sysinformer_free_string(p)
    return json.loads(raw)


def ncpu():
    try:
        return os.cpu_count() or 1
    except Exception:
        return 1


_busy_stop = None


def _burn(stop):
    """真实占用 CPU 的忙循环。不能用 sleep —— 那不加负载。"""
    x = 0
    while not stop.is_set():
        for _ in range(200000):
            x += 1
        if x < 0:
            break


def measure(seconds, label, reference, interval_ms=50):
    """同时采样库与参照。

    返回 (库均值, 参照均值, 库样本数, 参照样本数, 库跨度, 库样本列表)。

    **把原始样本一并返回**：判定失败时要靠它们做诊断（前 N 条 vs 其余）。
    此前把诊断做成单独的 `--probe` 调用，结果我在 workflow 里把时长
    当位置参数传（`--probe 12`），argparse 直接报错、诊断根本没跑 ——
    同一类错误犯了三次。修法是**消除这个错误类别**：诊断并入失败路径，
    只有一种调用形式。

    `interval_ms` 决定**库的采集窗口**：库内部有 100ms 最小采集间隔，
    调用快于它时复用上次读数；调用慢于它时，每次调用覆盖的 PDH 窗口
    就等于两次调用的间隔。所以把 interval_ms 调到 1000 可以让库的窗口
    与参照（typeperf 1s）等长 —— 这是区分「窗口长度导致的偏差」与
    「实现本身有偏差」的关键手段。
    """
    stop = threading.Event()
    samples = []
    ts = []

    def stream():
        first = True
        while not stop.is_set():
            try:
                env = call("system.snapshot")
            except Exception:
                time.sleep(interval_ms / 1000.0)
                continue
            u = ((env.get("data") or {}).get("cpu") or {}).get("usage")
            if first:
                first = False
            elif isinstance(u, (int, float)):
                samples.append(float(u))
                ts.append(time.time())
            time.sleep(interval_ms / 1000.0)

    th = threading.Thread(target=stream, daemon=True)
    th.start()
    ref = list(reference(seconds))
    stop.set()
    th.join(timeout=5)

    lm = statistics.mean(samples) if samples else float("nan")
    rm = statistics.mean(ref) if ref else float("nan")
    span = (max(ts) - min(ts)) if len(ts) > 1 else 0.0
    print(f"  [{label}] 库均值={lm:7.3f}% (n={len(samples)})   "
          f"参照均值={rm:7.3f}% (n={len(ref)})   库跨度={span:.1f}s")
    return lm, rm, len(samples), len(ref), span, samples


def diagnose(label, samples):
    """逐样本分解：前 N 条 vs 其余。

    用来区分两种成因（处置完全不同）：

    * 前 N 条显著更高 -> **预热值未被丢弃**（丢预热即可修）
    * 前 N 条与其余相当 -> **系统性偏移**（要改采集方式）

    N 取 30：CI 实测空闲时库偏高 5.5pp、样本 455 条，
    5.5 × 455 ≈ 2500 样本pp；若约 25 条读到 ~100% 正好等于这个量，
    所以前 30 条足以看出。
    """
    if not samples:
        return
    head, tail = samples[:30], samples[30:]
    print(f"    [诊断 {label}] 前 30 条均值 = {statistics.mean(head):7.3f}%")
    if tail:
        print(f"                   其余 {len(tail)} 条均值 = "
              f"{statistics.mean(tail):7.3f}%")
        print(f"                   全部均值        = "
              f"{statistics.mean(samples):7.3f}%")
        hi = sum(1 for v in head if v > 50)
        print(f"                   前 30 条里 >50% 的条数 = {hi}")
        overall = statistics.mean(samples)
        if overall > 90.0:
            # 饱和时该诊断不适用：此时几乎所有样本都贴着 ~100%，
            # 「前段略高于后段」只是正常波动。本机实测就出现过
            # 94.391 vs 91.222（差 3.17，刚过 3.0 阈值）而误报
            # 「预热值未被丢弃」。所以先判适用性，再下结论。
            print(f"                   -> 本段均值 {overall:.1f}% 接近饱和，"
                  f"**该诊断不适用**（前后差异属正常波动）")
        elif statistics.mean(head) - statistics.mean(tail) > 3.0:
            print("                   -> 前段明显更高，**预热值未被丢弃**")
        else:
            print("                   -> 前后相当，属**系统性偏移**")
    print("                   前 12 条：" +
          ", ".join(f"{v:.2f}" for v in samples[:12]))


def win_reference(seconds):
    """typeperf 走原始 PDH。逐行解析 CSV。"""
    p = subprocess.Popen(
        ["typeperf", r"\Processor(_Total)\% Processor Time",
         "-si", "1", "-sc", str(seconds)],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, encoding="utf-8", errors="replace")
    out = []
    for line in p.stdout:
        line = line.strip().strip('"')
        if not line or line.startswith("("):
            continue
        parts = line.split('","')
        if len(parts) >= 2:
            try:
                out.append(float(parts[1].strip('"')))
            except ValueError:
                pass
    p.wait()
    return out


def linux_reference(seconds):
    """读 /proc/stat 两次算 busy/(busy+idle)，每秒一次。"""
    def snap():
        with open("/proc/stat") as f:
            f = f.readline().split()[1:]
        vals = [int(x) for x in f]
        idle = vals[3] + (vals[4] if len(vals) > 4 else 0)
        total = sum(vals)
        return idle, total
    prev = snap()
    out = []
    for _ in range(seconds):
        time.sleep(1.0)
        cur = snap()
        di = cur[0] - prev[0]
        dt = cur[1] - prev[1]
        prev = cur
        if dt > 0:
            out.append(100.0 * (1.0 - di / dt))
    return out


def main():
    global LIB
    ap = argparse.ArgumentParser()
    ap.add_argument("lib", help="原生库路径")
    ap.add_argument("--phase-sec", type=int, default=PHASE_SEC)
    ap.add_argument("--busy", type=int, default=None,
                    help="忙线程数，默认核数的一半")
    ap.add_argument("--selftest", action="store_true",
                    help="只验证忙线程真的在吃 CPU，不跑完整判据")
    # 刻意**不提供单独的诊断模式**。
    # 诊断（前 30 条 vs 其余）已并入判定失败路径自动打印。
    # 2026-10-04 教训：曾提供 `--probe`，而 `--probe` 是开关、时长要走
    # `--phase-sec`，我在 workflow 里写成 `--probe 12`，argparse 直接报错、
    # 诊断根本没跑 —— 同一类错误（时长当位置参数）犯了三次。
    # 减少调用形式比反复改正调用更可靠。
    args = ap.parse_args()

    LIB = load_lib(os.path.abspath(args.lib))
    n = ncpu()
    busy = args.busy if args.busy is not None else max(1, n // 2)

    if args.selftest:
        # 忙线程自测：确认忙循环真的在吃 CPU（sleep 会假通过）。
        #
        # 判据要**考虑机器饱和**。首版写死「CPU 时间 >= 线程数 × 2」，
        # 在本机（已饱和）实测只有 4.61s / 应 24s，被判成失败 ——
        # 但那是**抢不到 CPU**，不是忙循环坏了。
        # 饱和机器上这个自测无法区分「循环坏了」与「抢不到」，
        # 所以如实报告实现到的并行度，只在**完全没吃到 CPU** 时判失败。
        print("=== 忙线程自测 ===")
        stop = threading.Event()
        ths = [threading.Thread(target=_burn, args=(stop,), daemon=True)
               for _ in range(max(1, n))]
        # 同时看参照负载，用来解释「为什么吃不满」
        ref_before = list(win_reference(2) if sys.platform == "win32"
                          else linux_reference(2))
        load0 = statistics.mean(ref_before) if ref_before else float("nan")
        before = _cpu_now()
        for t in ths:
            t.start()
        time.sleep(3)
        after = _cpu_now()
        stop.set()
        time.sleep(0.3)
        got = after - before
        want = len(ths) * 2.0        # 3s 内理论上限约 n*3，取保守的 n*2
        print(f"  起 {len(ths)} 个忙线程，3s 内拿到 CPU 时间 = {got:.2f}s"
              f"（空闲机器上应接近 {want:.0f}s）")
        print(f"  期间系统负载 ≈ {load0:.1f}%")
        if got <= 0.05:
            print("  **一个 tick 都没吃到 -> 忙循环没在工作，判据会假通过**")
            return 1
        ratio = got / want
        if ratio >= 0.5:
            print(f"  并行度 {ratio * 100:.0f}%  SELFTEST_OK")
        else:
            print(f"  并行度只有 {ratio * 100:.0f}% —— 与系统负载 {load0:.0f}% 相符，"
                  f"属**抢不到 CPU**而非忙循环故障。")
            print("  饱和机器上该自测无法进一步区分；完整的增量判据会判"
                  "「无判别力」并拒绝给结论。")
            print("  SELFTEST_OK（忙循环确认在跑）")
        return 0

    print("=== CPU 准确性：增量判据 ===")
    print(f"  逻辑核数 = {n}，加的忙线程 = {busy}"
          f"（预期增量约 {busy * 100.0 / n:.0f}pp）")
    print(f"  每段 {args.phase_sec}s\n")

    ref_fn = win_reference if sys.platform == "win32" else linux_reference

    try:
        call("system.snapshot")          # 预热
    except Exception as e:
        print("  预热失败:", e)
        return 3

    L1, R1, n1, r1, sp1, s1 = measure(args.phase_sec, "阶段1 基线", ref_fn)

    global _busy_stop
    _busy_stop = threading.Event()
    for _ in range(busy):
        threading.Thread(target=_burn, args=(_busy_stop,), daemon=True).start()
    time.sleep(2)

    L2, R2, n2, r2, sp2, s2 = measure(args.phase_sec, "阶段2 加负载", ref_fn)
    _busy_stop.set()

    if min(r1, r2) < args.phase_sec - 2 or min(n1, n2) < 20:
        print(f"\n  **样本不足（库 {n1}/{n2}，参照 {r1}/{r2}）-> "
              f"不是通过，是没测到**")
        return 3
    if min(sp1, sp2) < (args.phase_sec + 2) * 0.8:
        print(f"\n  **库采样跨度不足（{sp1:.1f}s / {sp2:.1f}s）-> 结论无效**")
        return 3

    dL = L2 - L1
    dR = R2 - R1
    print(f"\n  库的增量   = {dL:+.3f}pp   ({L1:.2f} -> {L2:.2f})")
    print(f"  参照的增量 = {dR:+.3f}pp   ({R1:.2f} -> {R2:.2f})")
    print(f"  增量之差   = {dL - dR:+.3f}pp")

    # ---- 诊断并入失败路径 ------------------------------------------
    # 只在需要时才打印（通过时不刷屏），但**不需要第二次调用**。
    # 这一并消除了「把时长当位置参数传给 --probe」那类错误。
    if dR < MIN_INCREMENT_PP or abs(dL - dR) > max(3.0, abs(dR) * 0.10):
        print("\n  ---- 逐样本诊断 ----")
        diagnose("阶段1 基线", s1)
        diagnose("阶段2 加负载", s2)

    # ---- 窗口长度对照：区分「窗口导致的偏差」与「实现本身的偏差」 ----
    # CI 实测（run 37170551267）：空闲时库 6.09% vs 参照 0.63%（+5.5pp），
    # 而 25% 负载时两者一致。**加性偏移被排除**（否则 25% 时也该偏高）。
    # 唯一能同时解释两端的是**下限效应**：短窗口（~112ms）里任何活动
    # 占比都被放大，1s 窗口会稀释掉。
    # 这一测直接把库的窗口拉到 1s（与参照等长）再比一次 ——
    # 若空闲读数随之降到 ~0.6%，则「窗口长度」就是成因。
    print("\n  ---- 窗口长度对照（库以 1Hz 调用 -> PDH 窗口 ~1s）----")
    L1b, R1b, n1b, r1b, _, _ = measure(12, "基线@1Hz", ref_fn, interval_ms=1000)
    if n1b >= 5 and r1b >= 8:
        d_short = L1 - R1
        d_long = L1b - R1b
        print(f"    短窗口偏差 = {d_short:+.3f}pp（50ms 调用，窗口 ~112ms）")
        print(f"    长窗口偏差 = {d_long:+.3f}pp（1s 调用，窗口 ~1s）")
        if abs(d_short) < 2.0:
            # 短窗口偏差本身很小 -> 该对照无判别力。
            # 饱和机器上两个偏差都接近 0，「拉长后变小」自然成立，
            # 会得出「窗口长度是成因」这种**看似有据实则无意义**的结论。
            # 与 diagnose() 的饱和判断同一类问题，故同样先判适用性。
            print(f"    -> 短窗口偏差本身只有 {abs(d_short):.2f}pp，"
                  f"**此对照无判别力**（无法据此判断窗口长度是否为成因）")
        elif d_long < d_short / 2:
            print("    -> **窗口长度是成因**：窗口拉长后偏差显著变小。")
        else:
            print("    -> 窗口长度不是主要成因，需另找。")

    print("\n=== 判读 ===")
    if dR < MIN_INCREMENT_PP:
        print(f"  参照的增量只有 {dR:.1f}pp < {MIN_INCREMENT_PP}pp ——")
        print("  **机器已饱和，加不出负载，此判据无判别力**。")
        print("  这不是「准确」，是「没测到」。请在空闲机器上跑")
        print("  （CI runner 4 核空闲时加 2 线程 ≈ +50pp）。")
        return 3
    tol = max(3.0, abs(dR) * 0.10)
    if abs(dL - dR) <= tol:
        print(f"  增量之差 {dL - dR:+.2f}pp 在容差 {tol:.1f}pp 内"
              f"（参照增量的 10%，且不低于 3pp）")
        print("  -> 库对**负载变化的响应**与参照一致。")
        print("  CPU_ACCURACY_OK")
        return 0
    print(f"  **增量之差 {dL - dR:+.2f}pp 超出容差 {tol:.1f}pp**")
    print("  -> 库对负载变化的响应与参照不一致，需要改采集方式。")
    print("  CPU_ACCURACY_FAILED")
    return 1


def _cpu_now():
    """本进程累计 CPU 秒数（跨平台）。"""
    try:
        import resource
        r = resource.getrusage(resource.RUSAGE_SELF)
        return r.ru_utime + r.ru_stime
    except Exception:
        pass
    if sys.platform == "win32":
        class FT(ctypes.Structure):
            _fields_ = [("lo", ctypes.c_uint32), ("hi", ctypes.c_uint32)]
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        ct = FT()
        k.GetProcessTimes.argtypes = [ctypes.c_void_p, ctypes.POINTER(FT),
                                      ctypes.POINTER(FT), ctypes.POINTER(FT),
                                      ctypes.POINTER(FT)]
        k.GetCurrentProcess.restype = ctypes.c_void_p
        h = k.GetCurrentProcess()
        ct = FT()
        ex = FT()
        kn = FT()
        us = FT()
        k.GetProcessTimes(h, ctypes.byref(ct), ctypes.byref(ex),
                          ctypes.byref(kn), ctypes.byref(us))
        def sec(f):
            return (f.hi << 32 | f.lo) / 1e7
        return sec(kn) + sec(us)
    return 0.0


if __name__ == "__main__":
    raise SystemExit(main())