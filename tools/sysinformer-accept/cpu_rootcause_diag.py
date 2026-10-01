#!/usr/bin/env python3
"""Windows CPU 口径根因诊断（临时脚本，定位后删除）。

## 背景

CI 的 4 核 runner 上，本库 `cpu.usage` 稳定在 ~20.2%（方差极小），
.NET `PerformanceCounter` 参考却是 ~2.0%（0.00~4.83% 波动），
差 +18.5pp。本机 12 核上两者差 <3pp。

已排除：核数不匹配（.NET/PDH/WMI 全 = 4）、参考源选错、PDH 实例子集取错、
库内口径混用（CI 日志「cpu.usage − 每核均值」平均 -0.43pp、非恒 0，
证明走的是 PDH 路径而非 sysinfo 回退）、PDH 侧不自洽。

## 为什么必须在 CI 上跑

关键条件是**低负载**。本机已被其它会话占满（起 1 个忙等进程时三列
都读 ~100%），造不出「真值 = k/ncpu」的可控条件，因此本机无法复现，
只能到出问题的机器上量。

## 五个数据源

  A  本库         system.snapshot 的 cpu.usage 与每核
  B  裸 PDH       ctypes 直读，Python 侧独立复刻 cpu_windows.rs
  C  .NET         PerformanceCounter（pdh_stream.ps1）
  D  typeperf     Windows 自带 CLI，另一条代码路径
  E  WMI          Win32_Processor LoadPercentage，完全不同的栈

B/C/D 都走 PDH，E 走 WMI。若 A≈B≈D 而 C/E 都低，则说明
**`PdhGetFormattedCounterValue` 的瞬时值与 PerformanceCounter 的 cooked
值口径不同**；若 A≈B 而 C≈D≈E，则说明裸 PDH 瞬时值本身有问题。

B 额外报告两个此前从未被观测过的东西：

  * `PdhCollectQueryData` 的**返回码** —— `cpu_windows.rs:97` 把它丢弃了。
    若它在 CI 上失败，读到的是上一次成功采集的陈旧值，而这一点用任何
    现有观测都看不出来（数值本身不会报错）。
  * 每次读取的 `CStatus`。

## 用法

    python cpu_rootcause_diag.py <dll> [seconds]

在 CI 里以非阻断步骤运行（`continue-on-error` 语义）：它是诊断，
失败不该让整条腿变红，但**报告里必须能看到它的输出**。
"""
from __future__ import annotations

import ctypes
import json
import os
import re
import statistics
import subprocess
import sys
import tempfile
import time
from ctypes import wintypes

DLL = sys.argv[1] if len(sys.argv) > 1 else (
    r"D:\ch\project\utils-support-native-parent"
    r"\utils-support-native-sysinformer\src\main\resources\native"
    r"\windows-x86_64\sysinformer.dll")
SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 40

PWSH = r"C:\Program Files\PowerShell\7\pwsh.exe"

# 脚本所在目录 —— 不能用硬编码的本地绝对路径。
# 2026-10-02 实测踩到：本脚本原先指向 D:\ch\project\...，在 CI runner 上
# 不存在，参考流直接启动失败、`.NET` 那一列**静默变成「无样本」**，
# 而整体仍然「跑完了」。少一个数据源的诊断比崩掉更危险。
_HERE = os.path.dirname(os.path.abspath(__file__))
PS = os.path.join(_HERE, "pdh_stream.ps1")
if not os.path.isfile(PS):
    PS = os.path.join(_HERE, "..", "tools", "sysinformer-accept", "pdh_stream.ps1")

STREAM = os.path.join(tempfile.gettempdir(), "pdh_diag.jsonl")
TYPEPERF_TXT = os.path.join(tempfile.gettempdir(), "typeperf_diag.txt")

# 与 cpu_windows.rs 的 MIN_COLLECT_INTERVAL_MS 保持一致
MIN_COLLECT_MS = 100

PDH_FMT_DOUBLE = 0x200

# PDH 状态码取自 Windows SDK 的 um/pdhmsg.h（本机 10.0.17134.0 已核对）。
# 之前凭记忆写的两个常量是错的（INVALID_DATA 写成 0xC0000006、
# NO_COUNTER 写成 0xC0000BB3，真值分别是 0xC0000BBA 与 0xC0000BB9），
# 那样会把「无效」误标成「未知码」，掩盖真正的失败原因。
CSTATUS_VALID = 0x00000000
CSTATUS_NEW_DATA = 0x00000001
CSTATUS_NO_MACHINE = 0x800007D0
CSTATUS_NO_INSTANCE = 0x800007D1
CSTATUS_MORE_DATA = 0x800007D2
CSTATUS_ITEM_NOT_VALIDATED = 0x800007D3
CSTATUS_RETRY = 0x800007D4
CSTATUS_NO_DATA = 0x800007D5
# 关键的一个：分母非正 = 两次采样时间戳没有正向推进。
# cpu_windows.rs 只检查 API 返回码、**不检查 CStatus**，
# 所以这种读数会被当成真值使用。它也最能解释「数值稳定得反常」。
CSTATUS_NEGATIVE_DENOM = 0x800007D6
CSTATUS_NEGATIVE_TIMEBASE = 0x800007D7
CSTATUS_NO_OBJECT = 0xC0000BB8
CSTATUS_NO_COUNTER = 0xC0000BB9
CSTATUS_INVALID_DATA = 0xC0000BBA


def rcname(rc):
    if isinstance(rc, int):
        rc &= 0xFFFFFFFF
    return {
        0: "SUCCESS",
        CSTATUS_VALID: "VALID_DATA",
        CSTATUS_NEW_DATA: "NEW_DATA",
        CSTATUS_NO_MACHINE: "NO_MACHINE",
        CSTATUS_NO_INSTANCE: "NO_INSTANCE",
        CSTATUS_MORE_DATA: "MORE_DATA",
        CSTATUS_ITEM_NOT_VALIDATED: "ITEM_NOT_VALIDATED",
        CSTATUS_RETRY: "RETRY",
        CSTATUS_NO_DATA: "NO_DATA",
        CSTATUS_NEGATIVE_DENOM: "CALC_NEGATIVE_DENOMINATOR",
        CSTATUS_NEGATIVE_TIMEBASE: "CALC_NEGATIVE_TIMEBASE",
        CSTATUS_NO_OBJECT: "NO_OBJECT",
        CSTATUS_NO_COUNTER: "NO_COUNTER",
        CSTATUS_INVALID_DATA: "INVALID_DATA",
        0xC0000BBC: "INVALID_HANDLE",
        0xC0000BBD: "INVALID_ARGUMENT",
        0x80004005: "E_FAIL",
    }.get(rc, f"0x{rc:08X}" if isinstance(rc, int) else str(rc))

# FILETIME 是 100ns 单位
FILETIME = wintypes.FILETIME


def proc_cpu_seconds():
    """本进程已消耗的 CPU 秒数（用户态 + 内核态）。

    用途：量出「测量装置自己」占了多少系统 CPU。若本库报出的 20% 里
    有 18pp 是这个采样循环自己烧掉的，那问题不在口径而在**自扰动** ——
    参考源用 1s 窗口摊薄了这项开销，所以两者必然对不上。
    """
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.GetProcessTimes.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(FILETIME), ctypes.POINTER(FILETIME),
        ctypes.POINTER(FILETIME), ctypes.POINTER(FILETIME)]
    creation, exit_, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
    if not k32.GetProcessTimes(
            ctypes.windll.kernel32.GetCurrentProcess(),
            ctypes.byref(creation), ctypes.byref(exit_),
            ctypes.byref(kernel), ctypes.byref(user)):
        return None

    def hi_lo(ft):
        return (ft.dwHighDateTime << 32) | ft.dwLowDateTime

    return (hi_lo(kernel) + hi_lo(user)) / 1e7


class PDH_FMT_COUNTERVALUE(ctypes.Structure):
    # 64 位对齐：doubleValue 落在偏移 8
    _fields_ = [("CStatus", wintypes.DWORD), ("pad", wintypes.DWORD),
                ("doubleValue", ctypes.c_double)]


class RawPdh:
    """裸 PDH 读取器，与 cpu_windows.rs 同样注册 _Total + 每核。"""

    def __init__(self, ncpu):
        self.d = ctypes.WinDLL("pdh.dll")
        for fn, args in (
            ("PdhOpenQueryW", [wintypes.LPCWSTR, ctypes.c_size_t,
                               ctypes.POINTER(ctypes.c_void_p)]),
            ("PdhAddEnglishCounterW", [ctypes.c_void_p, wintypes.LPCWSTR,
                                      ctypes.c_size_t,
                                      ctypes.POINTER(ctypes.c_void_p)]),
            ("PdhCollectQueryData", [ctypes.c_void_p]),
            ("PdhGetFormattedCounterValue",
             [ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p,
              ctypes.POINTER(PDH_FMT_COUNTERVALUE)]),
            ("PdhCloseQuery", [ctypes.c_void_p]),
        ):
            getattr(self.d, fn).argtypes = args

        self.q = ctypes.c_void_p()
        rc = self.d.PdhOpenQueryW(None, 0, ctypes.byref(self.q))
        self.open_rc = rc
        self.counters = []
        self.add_rcs = []
        paths = [r"\Processor(_Total)\% Processor Time"]
        paths += [rf"\Processor({i})\% Processor Time" for i in range(ncpu)]
        for p in paths:
            c = ctypes.c_void_p()
            rc = self.d.PdhAddEnglishCounterW(self.q, p, 0, ctypes.byref(c))
            self.add_rcs.append((p, rc))
            if rc == 0:
                self.counters.append(c)
        # 建立基线，与 cpu_windows.rs 一致：连采两次、间隔 120ms
        self.warm_rcs = []
        self.warm_rcs.append(self.d.PdhCollectQueryData(self.q))
        time.sleep(0.12)
        self.warm_rcs.append(self.d.PdhCollectQueryData(self.q))
        # sample_floored 的复用缓存：(时刻, 汇总, 每核)
        self.last_ok = None

    def sample(self):
        """返回 (total, cores, collect_rc, [(cstatus, value)...])"""
        rc = self.d.PdhCollectQueryData(self.q)
        vals = []
        for c in self.counters:
            v = PDH_FMT_COUNTERVALUE()
            r = self.d.PdhGetFormattedCounterValue(
                c, PDH_FMT_DOUBLE, None, ctypes.byref(v))
            vals.append((v.CStatus, None if r != 0 else v.doubleValue))
        total = vals[0][1] if vals else None
        # 过滤 None：PDH 偶发返回非 SUCCESS 时值是 None，混进求和会 TypeError，
        # 而一次偶发失败不该让整个诊断崩掉（要崩的话日志里就少了其它四个源）
        cores = [v for _, v in vals[1:] if v is not None]
        return total, cores, rc, vals

    def sample_floored(self):
        """复刻**修复后**的库逻辑：100ms 下限 + CStatus 检查 + 复用。

        用来回答一个关键问题：残余偏差到底是「库的缺陷」还是
        「该机器上这个窗口长度的固有测量特性」。若本列 ≈ A 本库，
        而两者都远高于 typeperf/WMI，则偏差来自窗口而非实现。
        """
        now = time.perf_counter()
        if self.last_ok is not None and \
                (now - self.last_ok[0]) * 1000 < MIN_COLLECT_MS:
            return self.last_ok[1], self.last_ok[2], "reuse"
        rc = self.d.PdhCollectQueryData(self.q)
        v = PDH_FMT_COUNTERVALUE()
        r = self.d.PdhGetFormattedCounterValue(
            self.counters[0], PDH_FMT_DOUBLE, None, ctypes.byref(v))
        if rc != 0 or r != 0 or v.CStatus != CSTATUS_VALID:
            if self.last_ok is not None:
                return self.last_ok[1], self.last_ok[2], "stale-reuse"
            return None, None, "no-value"
        total = v.doubleValue
        cores = []
        for c in self.counters[1:]:
            cv = PDH_FMT_COUNTERVALUE()
            cr = self.d.PdhGetFormattedCounterValue(
                c, PDH_FMT_DOUBLE, None, ctypes.byref(cv))
            if cr != 0 or cv.CStatus != CSTATUS_VALID:
                cores = None
                break
            cores.append(cv.doubleValue)
        if cores is None:
            if self.last_ok is not None:
                return self.last_ok[1], self.last_ok[2], "stale-reuse"
            return None, None, "partial-invalid"
        self.last_ok = (time.perf_counter(), total, list(cores))
        return total, cores, "collected"

    def close(self):
        self.d.PdhCloseQuery(self.q)


lib = ctypes.CDLL(DLL)
lib.sysinformer_call.restype = ctypes.c_void_p
lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]


def lib_cpu():
    p = lib.sysinformer_call(b"system.snapshot", b"{}")
    raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    lib.sysinformer_free_string(p)
    d = json.loads(raw)["data"]
    cores = [c["usage"] for c in d["cpu_cores"]]
    return float(d["cpu"]["usage"]), cores


def med(xs):
    return statistics.median(xs) if xs else float("nan")


def sd(xs):
    return statistics.pstdev(xs) if len(xs) > 1 else 0.0


def run_typeperf(out_path, secs):
    """typeperf 是 Windows 自带的另一条 PDH 代码路径。"""
    n = max(4, secs // 2)
    try:
        r = subprocess.run(
            ["typeperf", r"\Processor(_Total)\% Processor Time",
             "-si", "2", "-sc", str(n)],
            capture_output=True, timeout=secs + 90)
        txt = r.stdout.decode("utf-8", "replace") + \
            r.stderr.decode("utf-8", "replace")
        vals = []
        # typeperf 默认输出 CSV：'10/02/2026 00:04:30.104","76.837959'
        # 值在最后一个逗号之后。之前按「整行只有一个数字」匹配，
        # 结果一个样本都取不到（假「无样本」）。
        for line in txt.splitlines():
            if "," not in line:
                continue
            tail = line.rsplit(",", 1)[1].strip().strip('"')
            try:
                vals.append(float(tail))
            except ValueError:
                continue
        with open(out_path, "w", encoding="utf-8") as f:
            f.write(txt)
        return vals
    except Exception as e:
        return [f"typeperf 失败: {e}"]


def read_ref(path):
    rows = []
    if not os.path.exists(path):
        return rows
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ts, js = line.split("{", 1)
                d = json.loads("{" + js)
                rows.append((int(ts) / 1000.0, float(d.get("_total", 0.0)),
                             d))
            except Exception:
                pass
    return rows


def main():
    ncpu = ctypes.windll.kernel32.GetActiveProcessorCount(0xFFFF)
    print("=" * 72)
    print(f"  DLL      = {DLL}")
    print(f"  逻辑核数  = {ncpu}")
    print(f"  采样时长  = {SECS}s")
    print("=" * 72)

    # ---- 环境指纹：这些在 CI 上可能与本机不同 ----
    print("\n[0] 环境指纹")
    try:
        r = subprocess.run(["systeminfo"], capture_output=True, timeout=120)
        si = r.stdout.decode("utf-8", "replace") + \
            r.stderr.decode("utf-8", "replace")
        for key in ("OS Name", "OS Version", "System Manufacturer",
                    "System Model", "Hyper-V Requirements", "Virtualization"):
            for line in si.splitlines():
                if line.strip().startswith(key):
                    print("  " + line.strip()[:110])
                    break
    except Exception as e:
        print(f"  systeminfo 失败: {e}")
    for cmd, label in (
        (["wmic", "cpu", "get", "Name,NumberOfCores,NumberOfLogicalProcessors"],
         "WMI cpu"),
        (["wmic", "computersystem", "get", "model,manufacturer,hypervisorpresent"],
         "WMI system"),
    ):
        try:
            r = subprocess.run(cmd, capture_output=True, timeout=60)
            print(f"  {label}: "
                  f"{' '.join(r.stdout.decode('utf-8', 'replace').split())[:150]}")
        except Exception as e:
            print(f"  {label}: 失败 {e}")

    # ---- 裸 PDH 建查询 ----
    print("\n[1] 裸 PDH 查询建立情况")
    pdh = RawPdh(ncpu)
    # F 列用**独立**的查询，避免与 B 共用同一份 PDH 内部状态
    # （共用会让两个列的时间基准互相干扰，失去对照意义）
    pdh_floor = RawPdh(ncpu)
    print(f"  PdhOpenQueryW      rc=0x{pdh.open_rc:08X} {rcname(pdh.open_rc)}")
    bad = [(p, rc) for p, rc in pdh.add_rcs if rc != 0]
    print(f"  PdhAddEnglishCounter 成功 {len(pdh.counters)}/{len(pdh.add_rcs)}")
    for p, rc in bad[:8]:
        print(f"    失败 {p} rc=0x{rc:08X} {rcname(rc)}")
    print(f"  预热 PdhCollectQueryData rc = "
          f"{['0x%08X' % x for x in pdh.warm_rcs]}")

    # ---- 并发跑参考流 ----
    # 间隔取 100ms 而不是默认的 1000ms：**与库的最小采集窗口一致**。
    # 2026-10-02 实测的残余 +1.6pp，两侧四元组差几乎相等（即各自自洽），
    # 指向「测的不是同一段时间」。只有让两侧窗口**等长**，配对比较才能
    # 判定这 1.6pp 是库的偏差还是窗口长度差。
    # 用 1000ms 跑出来的差包含窗口失配，**不能**用来判库。
    if os.path.exists(STREAM):
        os.remove(STREAM)
    ref = subprocess.Popen(
        [PWSH, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", PS,
         "-Out", STREAM, "-DurationSec", str(SECS + 8),
         "-IntervalMs", str(MIN_COLLECT_MS)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    a_tot, a_core, b_tot, b_core, f_tot = [], [], [], [], []
    # 带时间戳的 A/F 样本，用于与同窗口的参考流配对
    a_pts, f_pts = [], []
    f_modes = {}
    collect_rcs, cstatus_hist = [], {}
    a_core_identical = 0
    a_core_identical_active = 0      # 逐核相同**且**不是全 idle
    samples = 0
    loop_cost_ms = []
    loop_errors = 0
    loop_err_first = ""
    t_wall0 = time.time()
    t_cpu0 = proc_cpu_seconds()
    t_end = t_wall0 + SECS
    try:
        while time.time() < t_end:
            t1 = time.perf_counter()
            # 单次迭代的任何异常都不能让整个诊断死掉。
            # 2026-10-02 实测踩到：CI 上 PdhGetFormattedCounterValue 偶发
            # 非 SUCCESS（同一条件即 CALC_NEGATIVE_DENOMINATOR），值是 None，
            # 混进 sum() 直接 TypeError，结果整份诊断只留下一条 warning、
            # 一个数据都没有 —— 而它本该回答的问题恰恰是「这个失败频率多高」。
            # 崩掉的诊断比没有诊断更糟：它把「没测到」伪装成「测过了」。
            try:
                at, ac = lib_cpu()
                bt, bc, crc, vals = pdh.sample()
                loop_cost_ms.append((time.perf_counter() - t1) * 1000)
                for cs, _v in vals:
                    cstatus_hist[cs] = cstatus_hist.get(cs, 0) + 1
                collect_rcs.append(crc)
                samples += 1
                if at is not None:
                    a_tot.append(at)
                    a_pts.append((time.time(), at))   # 秒：与 read_ref 的单位一致
                if ac:
                    a_core.append(sum(ac) / len(ac))
                    if len(set(ac)) == 1 and len(ac) > 1:
                        a_core_identical += 1
                        # 「四个核完全相同」只有在**不是全 idle** 时才说明问题。
                        # 2026-10-02 CI 上误报：空闲的 4 核 VM 上四个核本来
                        # 就都读 ~0，逐位相同是正常结果。原先的判据在这里
                        # 报「没有真正读到 per-instance 计数器」，是假警报。
                        if max(ac) > 1.0:
                            a_core_identical_active += 1
                # F 列：修复后逻辑的 Python 复刻
                ft, fc, fmode = pdh_floor.sample_floored()
                f_modes[fmode] = f_modes.get(fmode, 0) + 1
                if ft is not None:
                    f_tot.append(ft)
                    f_pts.append((time.time(), ft))   # 秒：与 read_ref 的单位一致
                if bt is not None:
                    b_tot.append(bt)
                if bc:
                    b_core.append(sum(bc) / len(bc))
            except Exception as e:
                loop_errors += 1
                if not loop_err_first:
                    loop_err_first = f"{type(e).__name__}: {e}"
            time.sleep(max(0.02 - (time.perf_counter() - t1), 0))
    finally:
        pdh.close()
        pdh_floor.close()
        t_wall1 = time.time()
        t_cpu1 = proc_cpu_seconds()
        out, err = ref.communicate(timeout=SECS + 120)

    if loop_errors:
        print(f"\n  [采样循环异常] {loop_errors}/{samples + loop_errors} 次迭代抛错，"
              f"首个 = {loop_err_first}")
        print("  这些迭代被跳过，不计入任何统计。异常本身也是信息："
              "它说明 PDH 读取在该机器上不总是成功。")
    if samples == 0:
        print("\n  !! 一次有效采样都没有，无法给出任何结论。"
              "这不是「通过」，是「没测到」。")

    # 自扰动：本采样循环自身吃掉了多少系统 CPU
    self_pct = None
    if t_cpu0 is not None and t_cpu1 is not None and t_wall1 > t_wall0:
        self_pct = (t_cpu1 - t_cpu0) / ((t_wall1 - t_wall0) * ncpu) * 100.0
    period_ms = med([(t_wall1 - t_wall0) / max(samples, 1) * 1000])

    # ---- typeperf 单独跑，避免与上面争 CPU ----
    tp = run_typeperf(TYPEPERF_TXT, SECS)

    # ---- 汇总 ----
    rows = read_ref(STREAM)
    c_ref = [v for _, v, _ in rows]

    print("\n[2] 五个数据源的中位数")
    print(f"  {'源':<10}{'中位数':>10}{'标准差':>10}{'最小':>10}{'最大':>10}{'样本':>8}")
    for label, xs in (("A 本库", a_tot), ("B 裸PDH", b_tot), ("C .NET", c_ref),
                      ("D typeperf", [x for x in tp if isinstance(x, float)]),
                      ("F 修复后逻辑", f_tot),
                      ("E WMI", [])):
        if xs:
            print(f"  {label:<10}{med(xs):>10.2f}{sd(xs):>10.2f}"
                  f"{min(xs):>10.2f}{max(xs):>10.2f}{len(xs):>8}")
        else:
            print(f"  {label:<10}{'(无样本)':>10}")

    ewmi = []
    try:
        r = subprocess.run(["wmic", "cpu", "get", "LoadPercentage"],
                           capture_output=True, timeout=60)
        for line in r.stdout.decode("utf-8", "replace").splitlines():
            m = re.match(r"^\s*(\d+)\s*$", line)
            if m:
                ewmi.append(float(m.group(1)))
    except Exception:
        pass
    if ewmi:
        print(f"  {'E WMI':<10}{med(ewmi):>10.2f}{sd(ewmi):>10.2f}"
              f"{min(ewmi):>10.2f}{max(ewmi):>10.2f}{len(ewmi):>8}")

    print("\n[3] 裸 PDH 的健康度")
    rc_bad = sum(1 for x in collect_rcs if x != 0)
    print(f"  PdhCollectQueryData 失败次数 = {rc_bad}/{len(collect_rcs)}")
    if rc_bad:
        from collections import Counter
        print(f"  失败返回码分布 = "
              f"{ {('0x%08X' % k): v for k, v in Counter(collect_rcs).items() if k != 0} }")
    print(f"  PdhGetFormattedCounterValue 的 CStatus 分布 = "
          f"{ {('0x%08X' % k): v for k, v in cstatus_hist.items()} }")
    print(f"  （0x{CSTATUS_VALID:08X}=VALID_DATA  "
          f"0x{CSTATUS_INVALID_DATA:08X}=INVALID_DATA  "
          f"0x{CSTATUS_NO_COUNTER:08X}=NO_COUNTER）")
    if any(k != CSTATUS_VALID for k in cstatus_hist):
        print("  !! 存在非 VALID 的 CStatus：读到的可能不是真实当前值")

    print("\n[4] 库内逐核")
    if samples:
        print(f"  逐核完全相同的采样 = {a_core_identical}/{samples}"
              f" ({a_core_identical * 100.0 / samples:.0f}%)")
        print(f"  其中**非全 idle**（最大值 >1%）的 = {a_core_identical_active}"
              f"/{samples} "
              f"({a_core_identical_active * 100.0 / samples:.0f}%)")
        print("  说明：空闲机器上四个核本来就都读 ~0，逐位相同是**正常**结果。")
        print("  只有「相同且不 idle」才说明 per-instance 读取有问题 —— "
              "2026-10-02 CI 上原判据在这里报了假警报。")
        if a_core_identical_active * 100.0 / samples > 50:
            print("  !! 多数采样逐核相同且不 idle：per-instance 读取可能有问题")
    else:
        print("  (无有效采样)")

    print("\n[5] 差值（相对各参照源）")
    for label, xs in (("A 本库", a_tot), ("B 裸PDH", b_tot),
                      ("F 修复后逻辑", f_tot)):
        if xs and c_ref:
            print(f"  {label} - C .NET = {med(xs) - med(c_ref):+.2f}pp")
    if a_tot and b_tot:
        print(f"  A - B      = {med(a_tot) - med(b_tot):+.2f}pp"
              f"  （B 是修复前的读法；两者接近说明库仍像旧行为）")
    if a_tot and f_tot:
        print(f"  A - F      = {med(a_tot) - med(f_tot):+.2f}pp"
              f"  （F 是修复后逻辑的 Python 复刻；≈0 说明库与修复实现一致）")
    if f_modes:
        print(f"  F 的行为分布 = {f_modes}")
        print("  （reuse = 未到最小间隔未采集；collected = 真正采集；"
              "stale-reuse = CStatus 无效、复用上一次）")
    ew = [x for x in tp if isinstance(x, float)]
    if ew and c_ref:
        print(f"  D - C      = {med(ew) - med(c_ref):+.2f}pp"
              f"  （≈0 说明 .NET 与 typeperf 同源一致）")
    if ew and a_tot:
        print(f"  A - D      = {med(a_tot) - med(ew):+.2f}pp")

    print("\n[6] 自扰动：本采样循环自身吃掉的系统 CPU")
    print(f"  循环周期      = {period_ms:.2f} ms（{samples} 次采样 / "
          f"{t_wall1 - t_wall0:.1f}s）")
    if loop_cost_ms:
        srt = sorted(loop_cost_ms)
        p95 = srt[min(int(len(srt) * 0.95), len(srt) - 1)]
        print(f"  单次迭代耗时  = p50 {med(loop_cost_ms):.2f} ms  p95 {p95:.2f} ms")
    else:
        print("  单次迭代耗时  = (无有效样本)")
    if self_pct is not None:
        print(f"  本进程 CPU 占用系统比例 = {self_pct:.2f}%")
        print(f"  （{ncpu} 核机器上，{self_pct:.2f}% 相当于 "
              f"{self_pct * ncpu / 100:.2f} 个核被测量装置本身占住）")
        excess = (med(a_tot) - med(c_ref)) if (a_tot and c_ref) else float("nan")
        if excess != excess:            # NaN
            pass
        elif excess <= 0:
            # 本库读数并不比参考高，自扰动解释不了「偏高」这件事
            print(f"  本库读数不比参考高（{excess:+.2f}pp），"
                  f"自扰动不构成差异来源")
        elif self_pct >= abs(excess) * 0.6:
            print(f"  本库比参考高 {excess:+.2f}pp，自扰动 {self_pct:.2f}pp "
                  f"-> 自扰动可解释大部分差异")
        else:
            print(f"  本库比参考高 {excess:+.2f}pp，自扰动只有 {self_pct:.2f}pp "
                  f"-> 自扰动不足以解释")
    else:
        print("  GetProcessTimes 不可用，无法量化自扰动")

    if any(k != CSTATUS_VALID for k in cstatus_hist):
        bad_total = sum(v for k, v in cstatus_hist.items() if k != CSTATUS_VALID)
        all_total = sum(cstatus_hist.values())
        print(f"\n[7] 非 VALID 的 CStatus 共 {bad_total}/{all_total} "
              f"({bad_total * 100.0 / all_total:.2f}%)：")
        for k, v in sorted(cstatus_hist.items(), key=lambda x: -x[1]):
            if k != CSTATUS_VALID:
                print(f"    0x{k:08X} {rcname(k):<28} x{v}")
        if CSTATUS_NEGATIVE_DENOM in cstatus_hist:
            print(f"  其中 PDH_CALC_NEGATIVE_DENOMINATOR 占 "
                  f"{cstatus_hist[CSTATUS_NEGATIVE_DENOM]} 次 —— "
                  f"两次采样时间戳没有正向推进，分母非正，doubleValue 无意义。")
            print(f"  这就是 2026-10-02 修复的根因：**修复前**的 cpu_windows.rs "
                  f"只检查 API 返回码、不检查 CStatus，这类读数会被原样当真值。")
            print(f"  现在库已检查 CStatus 并强制 {MIN_COLLECT_MS}ms 最小采集间隔。"
                  f"请对照 A 本库 与 F 修复后逻辑 两列：")
            print(f"    - A ≈ B（本脚本的裸查询，高速率）-> 库仍在按旧行为读")
            print(f"    - A ≈ F（修复后逻辑的复刻）      -> 库行为与修复一致")

    if err and err.strip():
        print(f"\n  [参考流 stderr] {err.strip()[:300]}")

    # ---- 同窗口配对比较（本脚本最有价值的一段）----
    if rows and a_pts:
        print(f"\n[8] 同窗口配对比较（参考流间隔 = "
              f"{MIN_COLLECT_MS}ms，与库的最小窗口等长）")

        def paired(pts, tol_ms):
            out = []
            rvals = [(t, v) for t, v, _ in rows]
            rts = [t for t, _ in rvals]
            for t, v in pts:
                # 线性找最近邻即可，样本量不大
                best, bestd = None, None
                lo, hi = 0, len(rts) - 1
                while lo <= hi:
                    mid = (lo + hi) // 2
                    d = abs(rts[mid] - t)
                    if bestd is None or d < bestd:
                        best, bestd = mid, d
                    if rts[mid] < t:
                        lo = mid + 1
                    else:
                        hi = mid - 1
                if best is not None and bestd is not None and bestd <= tol_ms:
                    out.append(v - rvals[best][1])
            return out

        tol = max(MIN_COLLECT_MS // 2, 60)
        for label, pts in (("A 本库", a_pts), ("F 修复后逻辑", f_pts)):
            ds = paired(pts, tol)
            if len(ds) < 5:
                print(f"  {label}: 可配对样本仅 {len(ds)}，不足（需要 >=5）")
                continue
            m = sum(ds) / len(ds)
            sd_ = (sum((x - m) ** 2 for x in ds) / (len(ds) - 1)) ** 0.5
            se = sd_ / (len(ds) ** 0.5)
            print(f"  {label:<12} 配对 {len(ds):>5}   均值差 = {m:+.3f}pp   "
                  f"标准差 = {sd_:.3f}pp   95% CI = "
                  f"[{m - 1.96 * se:+.3f}, {m + 1.96 * se:+.3f}]")
            if m - 1.96 * se <= 0 <= m + 1.96 * se:
                print(f"               -> 95% CI 含 0：**等长窗口下无系统性偏差**")
            else:
                print(f"               -> 95% CI 不含 0：等长窗口下仍有偏差")
        print(f"  配对容差 = ±{tol}ms（时间戳单位：秒）。参考流与本库进程**不是**同一个采样时刻，")
        print(f"  残余的毫秒级错配仍会带来几十 pp 的单点噪声，")
        print(f"  所以这里看的是**均值与置信区间**，不是逐点相等。")

    print("\n  DIAG_DONE")


if __name__ == "__main__":
    main()