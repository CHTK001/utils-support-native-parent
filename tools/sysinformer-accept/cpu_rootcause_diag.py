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
PS = (r"D:\ch\project\utils-support-native-parent\tools\sysinformer-accept"
      r"\pdh_stream.ps1")
STREAM = os.path.join(tempfile.gettempdir(), "pdh_diag.jsonl")
TYPEPERF_TXT = os.path.join(tempfile.gettempdir(), "typeperf_diag.txt")

PDH_FMT_DOUBLE = 0x200
CSTATUS_VALID = 0          # PDH_CSTATUS_VALID_DATA 就是 0
CSTATUS_INVALID_DATA = 0xC0000006
CSTATUS_NO_COUNTER = 0xC0000BB3


def rcname(rc):
    return {0: "SUCCESS",
            0xC0000006: "INVALID_DATA",
            0xC0000BB3: "CSTATUS_NO_COUNTER",
            0x80004005: "E_FAIL",
            0xC00000E5: "INVALID_ARGUMENT"}.get(rc, f"0x{rc & 0xFFFFFFFF:08X}")


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
        cores = [v for _, v in vals[1:]]
        return total, cores, rc, vals

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
    print(f"  PdhOpenQueryW      rc=0x{pdh.open_rc:08X} {rcname(pdh.open_rc)}")
    bad = [(p, rc) for p, rc in pdh.add_rcs if rc != 0]
    print(f"  PdhAddEnglishCounter 成功 {len(pdh.counters)}/{len(pdh.add_rcs)}")
    for p, rc in bad[:8]:
        print(f"    失败 {p} rc=0x{rc:08X} {rcname(rc)}")
    print(f"  预热 PdhCollectQueryData rc = "
          f"{['0x%08X' % x for x in pdh.warm_rcs]}")

    # ---- 并发跑参考流 ----
    if os.path.exists(STREAM):
        os.remove(STREAM)
    ref = subprocess.Popen(
        [PWSH, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", PS,
         "-Out", STREAM, "-DurationSec", str(SECS + 8)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    a_tot, a_core, b_tot, b_core = [], [], [], []
    collect_rcs, cstatus_hist = [], {}
    a_core_identical = 0
    samples = 0
    t_end = time.time() + SECS
    try:
        while time.time() < t_end:
            t1 = time.perf_counter()
            at, ac = lib_cpu()
            bt, bc, crc, vals = pdh.sample()
            for cs, _v in vals:
                cstatus_hist[cs] = cstatus_hist.get(cs, 0) + 1
            collect_rcs.append(crc)
            samples += 1
            if at is not None:
                a_tot.append(at)
            if ac:
                a_core.append(sum(ac) / len(ac))
                if len(set(ac)) == 1 and len(ac) > 1:
                    a_core_identical += 1
            if bt is not None:
                b_tot.append(bt)
            if bc:
                b_core.append(sum(bc) / len(bc))
            time.sleep(max(0.02 - (time.perf_counter() - t1), 0))
    finally:
        pdh.close()
        out, err = ref.communicate(timeout=SECS + 120)

    # ---- typeperf 单独跑，避免与上面争 CPU ----
    tp = run_typeperf(TYPEPERF_TXT, SECS)

    # ---- 汇总 ----
    rows = read_ref(STREAM)
    c_ref = [v for _, v, _ in rows]

    print("\n[2] 五个数据源的中位数")
    print(f"  {'源':<10}{'中位数':>10}{'标准差':>10}{'最小':>10}{'最大':>10}{'样本':>8}")
    for label, xs in (("A 本库", a_tot), ("B 裸PDH", b_tot), ("C .NET", c_ref),
                      ("D typeperf", [x for x in tp if isinstance(x, float)]),
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

    print("\n[3] 裸 PDH 的健康度（cpu_windows.rs 丢弃了 collect 返回码）")
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
    if a_core_identical and samples:
        print(f"  本库逐核**完全相同**的采样 = {a_core_identical}/{samples}"
              f" ({a_core_identical * 100.0 / samples:.0f}%)")
        if a_core_identical * 100.0 / samples > 80:
            print("  !! 超过 80% 的采样逐核完全相同：真实 CPU 不可能如此，")
            print("     说明库没有真正读到 per-instance 计数器")
    else:
        print(f"  本库逐核完全相同的采样 = {a_core_identical}/{samples}")

    print("\n[5] 差值（相对 .NET 参考）")
    for label, xs in (("A 本库", a_tot), ("B 裸PDH", b_tot)):
        if xs and c_ref:
            print(f"  {label} - C = {med(xs) - med(c_ref):+.2f}pp")
    if a_tot and b_tot:
        print(f"  A - B      = {med(a_tot) - med(b_tot):+.2f}pp"
              f"  （>1pp 说明库与独立裸 PDH 查询也不一致）")
    ew = [x for x in tp if isinstance(x, float)]
    if ew and c_ref:
        print(f"  D - C      = {med(ew) - med(c_ref):+.2f}pp"
              f"  （≈0 说明 .NET 与 typeperf 同源一致）")
    if ew and a_tot:
        print(f"  A - D      = {med(a_tot) - med(ew):+.2f}pp")

    if err and err.strip():
        print(f"\n  [参考流 stderr] {err.strip()[:300]}")
    print("\n  DIAG_DONE")


if __name__ == "__main__":
    main()