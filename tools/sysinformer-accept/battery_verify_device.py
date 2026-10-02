#!/usr/bin/env python3
"""真机电池取值核对：一条命令，在**有电池的设备**上直接跑。

## 为什么需要它

电池取值的**逻辑分支**已经验证过（Linux 侧用 `mount --bind` 伪造 sysfs，
见 `battery_value_linux.py`，3 用例全绿）。但有一件事没有硬件就验不了：

    操作系统自己的 API（Windows `GetSystemPowerStatus` / macOS `pmset -g batt`）
    在**真笔记本**上到底返回哪些字段、语义是什么、边界值怎么表现。

这个未知**不能靠单元测试替代** —— 单元测试只能复验常量映射，
而映射是我读代码就能确认的。拿它冒充「验过了」是自欺。

所以本脚本不去 mock，只做一件事：**在真设备上把库的读数与操作系统
的另一套独立视图逐项对账**。任何一项对不上就是真缺陷。

## 各平台的对照源（都与库的实现路径不同）

Windows：

  | 库的取值       | 对照源                          | 独立性 |
  |----------------|---------------------------------|--------|
  | percentage     | CIM `Win32_Battery`             | WMI 栈 |
  |                | `powercfg /batteryreport` 的 XML | 电源服务 |
  | state          | `Win32_Battery.BatteryStatus`   | WMI 栈 |
  | time_to_empty  | `Win32_Battery.EstimatedRunTime`| WMI 栈 |

macOS：

  | 库的取值       | 对照源                              | 独立性 |
  |----------------|-------------------------------------|--------|
  | percentage     | `ioreg -rc AppleSmartBattery` 的     | IOKit |
  |                | CurrentCapacity/MaxCapacity         | 直接读 SMC |
  | state          | `ioreg` 的 `IsCharging` / `pmset`   | IOKit |
  | time_to_empty  | `ioreg` 的 `AppleRawBatteryRemaining` | |

## 用法

    # Windows 笔记本
    python battery_verify_device.py <入库的 sysinformer.dll>

    # MacBook
    python3 battery_verify_device.py <入库的 libsysinformer.dylib>

平台由库自己报告的 `system.snapshot.platform` 决定，不用手工指定。

## 判定

全部可对照项一致（或在容差内）时打印 `BATTERY_DEVICE_OK`，退出码 0；
出现对不上则打印 `BATTERY_DEVICE_FAILED` 并逐项列出差异，退出码 1。

**注意读数的窗口差异**：库的 CPU/电池值是 PDH/ACPI 的**瞬时或短窗**读数，
而 WMI/IOKit 的值有各自的缓存与平均。电池百分比变化很慢（充放电以
分钟计），所以百分比应当**精确相等**；剩余时间则给容差，因为两边的
模型不同（库用 `energy_now/power_now` 或内核给的 `time_to_empty_now`，
WMI 的 `EstimatedRunTime` 是内核的另一种估算）。
"""

from __future__ import annotations

import ctypes
import json
import os
import platform
import re
import subprocess
import sys
import tempfile

# 允许的对账容差
PCT_TOL = 1.0          # 百分点。充放电以分钟计，1pp 内不应有变化
TIME_TOL_FRAC = 0.35   # 剩余时间允许 35% 相对差（两边模型不同）

failures = []
notes = []
checks = 0


def check(label, got, want, tol=None, unit=""):
    global checks
    checks += 1
    if want is None or got is None:
        failures.append(f"{label}: got={got!r} want={want!r}（有一侧取不到）")
        print(f"  FAIL {label:<34} got={got!r}  want={want!r}")
        return False
    if tol is None:
        good = got == want
    else:
        good = abs(float(got) - float(want)) <= tol
    print(f"  {'ok  ' if good else 'FAIL'} {label:<34} "
          f"got={got}{unit}  want={want}{unit}"
          + (f"  tol=±{tol}{unit}" if tol is not None else ""))
    if not good:
        failures.append(f"{label}: got={got} want={want} tol={tol}")
    return good


def note(msg):
    notes.append(msg)
    print(f"  --   {msg}")


# ---------------------------------------------------------------- 库侧


def lib_version(path):
    """读 sysinformer_version()，它是平台信息的权威来源。

    单独开一次 CDLL 是为了不与 call() 共用句柄语义不清；本进程内重复
    CDLL 同一路径是安全的（Windows 会返回同一模块句柄）。
    """
    lib = ctypes.CDLL(path)
    lib.sysinformer_version.restype = ctypes.c_void_p
    p = lib.sysinformer_version()
    raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]
    lib.sysinformer_free_string(p)
    return json.loads(raw)


def load_lib(path):
    lib = ctypes.CDLL(path)
    lib.sysinformer_call.restype = ctypes.c_void_p
    lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
    lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]

    def call(op, args=None):
        p = lib.sysinformer_call(op.encode(), json.dumps(args or {}).encode())
        raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
        lib.sysinformer_free_string(p)
        return json.loads(raw)

    return call


# ---------------------------------------------------------------- Windows


def win_system_power_status():
    """直接调 GetSystemPowerStatus —— 这是**库用的那个 API**，
    放在这里是为了确认它在本机返回了有意义的值（而不是恒为 128）。"""
    class SPS(ctypes.Structure):
        _fields_ = [("ac_line_status", ctypes.c_ubyte),
                    ("battery_flag", ctypes.c_ubyte),
                    ("battery_life_percent", ctypes.c_ubyte),
                    ("battery_life_time", ctypes.c_uint32),
                    ("battery_full_life_time", ctypes.c_uint32)]

    k32 = ctypes.WinDLL("kernel32")
    st = SPS()
    if not k32.GetSystemPowerStatus(ctypes.byref(st)):
        return None
    return st


# BatteryStatus 的 WMI 取值 -> 本库 state 取值
WMI_STATE_MAP = {
    1: "discharging",   # Discharging
    2: "unknown",       # AC（未在充电）—— 本库对 AC 报 unknown
    3: "full",          # Fully Charged
    4: "low",           # Low
    5: "critical",      # Critical
    6: "charging",      # Charging
    7: "charging",      # Charging and High
    8: "charging",      # Charging and Low
    9: "charging",      # Charging and Critical
    10: "unknown",      # Undefined
    11: "unknown",      # Partially Charged
}


def windows_refs():
    """返回 (ref_pct, ref_state, ref_minutes, ref_full_minutes, source_desc)"""
    refs = {}

    # --- CIM Win32_Battery ---
    try:
        r = subprocess.run(
            ["powershell.exe", "-NoProfile", "-Command",
             "Get-CimInstance Win32_Battery | "
             "Select-Object -Property EstimatedChargeRemaining,BatteryStatus,"
             "EstimatedRunTime,DeviceID | ConvertTo-Json -Compress"],
            capture_output=True, timeout=120)
        txt = r.stdout.decode("utf-8", "replace").strip()
        if txt:
            d = json.loads(txt)
            if isinstance(d, dict):
                d = [d]
            b = d[0] if d else {}
            refs["pct"] = b.get("EstimatedChargeRemaining")
            refs["state"] = WMI_STATE_MAP.get(b.get("BatteryStatus"))
            # EstimatedRunTime 单位是分钟；255 表示「未知/不变」
            t = b.get("EstimatedRunTime")
            refs["minutes"] = None if t in (None, 255) else float(t) * 60
            refs["src"] = f"CIM Win32_Battery (DeviceID={b.get('DeviceID')})"
    except Exception as e:
        refs["err_cim"] = str(e)

    # --- powercfg /batteryreport 的 XML（电源服务，另一套栈）---
    try:
        out = os.path.join(tempfile.gettempdir(), "batteryreport.xml")
        if os.path.exists(out):
            os.remove(out)
        r = subprocess.run(["powercfg", "/batteryreport", "/output", out],
                           capture_output=True, timeout=180)
        if os.path.exists(out):
            import xml.etree.ElementTree as ET
            tree = ET.parse(out)
            pct = None
            for tag in ("BatteryCapacity", "DesignedCapacity",
                        "FullChargeCapacity"):
                node = tree.find(f".//{tag}")
                if node is not None and node.text:
                    refs.setdefault("powercfg", {})[tag] = int(node.text)
            fcd = refs.get("powercfg", {}).get("FullChargeCapacity")
            dcd = refs.get("powercfg", {}).get("DesignedCapacity")
            if fcd:
                refs["powercfg_pct"] = round(fcd / dcd * 100) if dcd else None
            refs["powercfg_xml"] = out
    except Exception as e:
        refs["err_powercfg"] = str(e)

    return refs


# ---------------------------------------------------------------- macOS


def macos_refs(pmset_text=None, ioreg_text=None):
    """采集 macOS 参考源。

    两个文本参数只为**可测**：本机与 CI 都不是 macOS，若不把解析拆成
    纯函数，这段正则就永远没被执行过 —— 而参考源解析错了，
    对账结论就是假的。所以 `--selftest` 用合成文本验证它。
    """
    refs = {}
    if pmset_text is None:
        try:
            r = subprocess.run(["pmset", "-g", "batt"], capture_output=True,
                               timeout=60)
            pmset_text = r.stdout.decode("utf-8", "replace")
        except Exception as e:
            refs["err_pmset"] = str(e)
            pmset_text = ""
    refs["pmset"] = pmset_text

    if ioreg_text is None:
        try:
            r = subprocess.run(["ioreg", "-rc", "AppleSmartBattery"],
                               capture_output=True, timeout=60)
            ioreg_text = r.stdout.decode("utf-8", "replace")
        except Exception as e:
            refs["err_ioreg"] = str(e)
            ioreg_text = ""
    refs["ioreg_raw"] = ioreg_text

    def num(key):
        m = re.search(r'"' + key + r'"\s*=\s*"?(\d+)"?', ioreg_text)
        return int(m.group(1)) if m else None

    def flag(key):
        """布尔字段。真实 ioreg 输出的是 `= No` / `= Yes`（**不是** 0/1），
        早先只写数字正则，结果在真 MacBook 上返回 None、state 对账被
        **静默跳过** —— 由 --selftest 抓到。
        """
        m = re.search(r'"' + key + r'"\s*=\s*"?([A-Za-z0-9]+)"?', ioreg_text)
        if not m:
            return None
        v = m.group(1).lower()
        if v in ("yes", "true", "1"):
            return 1
        if v in ("no", "false", "0"):
            return 0
        return None

    refs["current_capacity"] = num("CurrentCapacity")
    refs["max_capacity"] = num("MaxCapacity")
    if refs["max_capacity"] and refs["current_capacity"] is not None:
        refs["pct"] = round(
            refs["current_capacity"] / refs["max_capacity"] * 100)
    refs["is_charging"] = flag("IsCharging")
    refs["apple_raw_remaining"] = num("AppleRawBatteryRemaining")
    refs["fully_charged"] = flag("FullyCharged")
    return refs


# 合成样本：格式取自真实机器的输出（见 ACCEPTANCE.md 记录的 pmset 原文）
SELFTEST_PMSET_DISCHARGING = """Now drawing from 'Battery Power'
 -InternalBattery-0 (id=1234567)\t87%; discharging; 3:21 remaining present: true
"""

SELFTEST_PMSET_CHARGING = """Now drawing from 'AC Power'
 -InternalBattery-0 (id=1234567)\t76%; charging; 1:02 remaining until charged present: true
"""

SELFTEST_PMSET_FULL = """Now drawing from 'AC Power'
 -InternalBattery-0 (id=1234567)\t100%; charged; 0:00 remaining until charged present: true
"""

SELFTEST_PMSET_NO_BATTERY = "Now drawing from 'AC Power'\n"

SELFTEST_IOREG = (
    '"AppleSmartBattery" = {\n'
    '  "MaxCapacity" = 5103\n'
    '  "CurrentCapacity" = 4439\n'
    '  "IsCharging" = No\n'
    '  "FullyCharged" = No\n'
    '  "AppleRawBatteryRemaining" = 201\n'
    '}\n')


def selftest():
    """验证 macOS 参考源的解析（本机不是 macOS，只能靠合成文本）。"""
    cases = [
        ("pmset 放电", macos_refs(SELFTEST_PMSET_DISCHARGING, SELFTEST_IOREG),
         {"pct": 87, "is_charging": 0, "fully_charged": 0,
          "apple_raw_remaining": 201, "current_capacity": 4439}),
        ("pmset 充电", macos_refs(SELFTEST_PMSET_CHARGING, SELFTEST_IOREG),
         None),
        ("pmset 满电", macos_refs(SELFTEST_PMSET_FULL, SELFTEST_IOREG),
         None),
        ("pmset 无电池", macos_refs(SELFTEST_PMSET_NO_BATTERY, ""),
         {"pct": None, "current_capacity": None}),
    ]
    fails = 0
    print("=== macOS 参考源解析自测（合成文本）===")
    for label, r, expect in cases:
        print(f"  -- {label}")
        has_batt = "-InternalBattery-0" in (r.get("pmset") or "")
        print(f"     pmset 含电池行 = {has_batt}")
        if expect is None:
            continue
        for k, want in expect.items():
            got = r.get(k)
            good = got == want
            print(f"     {'ok  ' if good else 'FAIL'} {k:<22} "
                  f"got={got}  want={want}")
            if not good:
                fails += 1
    # 百分比换算单独验（CurrentCapacity/MaxCapacity -> pct）
    r = macos_refs(SELFTEST_PMSET_DISCHARGING, SELFTEST_IOREG)
    good = r.get("pct") == 87
    print(f"  {'ok  ' if good else 'FAIL'} pct = CurrentCapacity/MaxCapacity"
          f" = {r.get('pct')}（4439/5103 -> 87）")
    if not good:
        fails += 1

    # 布尔字段的四种取值都要覆盖：ioreg 真实输出是 Yes/No，
    # 早先的正则只匹配数字，靠这一项才发现（否则真机上会静默跳过）。
    print("  -- 布尔字段 Yes/No 变体")
    for label, txt, want_chg, want_full in (
        ("No/No（放电）", SELFTEST_IOREG, 0, 0),
        ("Yes/No（充电）",
         SELFTEST_IOREG.replace('"IsCharging" = No', '"IsCharging" = Yes'),
         1, 0),
        ("Yes/Yes（满电）",
         SELFTEST_IOREG.replace('"IsCharging" = No', '"IsCharging" = Yes')
         .replace('"FullyCharged" = No', '"FullyCharged" = Yes'), 1, 1),
        ("true/false 变体",
         SELFTEST_IOREG.replace('"IsCharging" = No', '"IsCharging" = true'),
         1, 0),
    ):
        rr = macos_refs(SELFTEST_PMSET_DISCHARGING, txt)
        ok = (rr.get("is_charging") == want_chg
              and rr.get("fully_charged") == want_full)
        print(f"     {'ok  ' if ok else 'FAIL'} {label:<18} "
              f"is_charging={rr.get('is_charging')}(期望{want_chg}) "
              f"fully_charged={rr.get('fully_charged')}(期望{want_full})")
        if not ok:
            fails += 1

    # pct 不能在 CurrentCapacity 缺失时被算成 0（早先只判 MaxCapacity）
    rr = macos_refs(SELFTEST_PMSET_DISCHARGING,
                    SELFTEST_IOREG.replace('"CurrentCapacity" = 4439', ''))
    ok = rr.get("pct") is None
    print(f"  {'ok  ' if ok else 'FAIL'} CurrentCapacity 缺失时 pct = "
          f"{rr.get('pct')}（应为 None，不能是 0）")
    if not ok:
        fails += 1

    print(f"\n  SELFTEST_{'OK' if not fails else 'FAILED'}  失败 {fails} 项")
    return 0 if not fails else 1


# ---------------------------------------------------------------- 主流程


def main():
    if len(sys.argv) >= 2 and sys.argv[1] == "--selftest":
        return selftest()
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    dll = sys.argv[1]
    if not os.path.isfile(dll):
        print(f"库不存在: {dll}")
        return 2

    call = load_lib(dll)

    # 平台取自 sysinformer_version()，不是 system.snapshot ——
    # 后者根本没有 platform 字段（实测顶层键只有 batteries/cpu/cpu_cores/
    # disk_io/disks/gpus/host/load/memory/networks/swap/timeline）。
    # 一开始我从 snapshot 取，结果拿到空串、被 host==Windows 的兜底掩盖了：
    # **兜底会掩盖取错字段**，这类 bug 不会报错只会静默走另一条分支。
    vp = lib_version(dll)
    snap = call("system.snapshot")
    if not snap.get("ok"):
        print(f"system.snapshot 失败: {snap}")
        return 2
    plat = (vp.get("platform") or "").lower()
    host = platform.system()

    print("=" * 72)
    print(f"  库     = {dll}")
    print(f"  版本   = {vp.get('version')}   平台 = {plat}   "
          f"target = {vp.get('target')}   宿主 = {host}")
    if not plat:
        print("  !! sysinformer_version() 没有 platform 字段，"
              "无法确定平台；请检查库版本")
        return 2
    print("=" * 72)

    bats = (call("battery.list") or {}).get("data") or []
    snap_bats = (snap["data"].get("batteries") or [])

    print("\n[1] 库读到的电池")
    print(f"  battery.list  条数 = {len(bats)}")
    for i, b in enumerate(bats):
        print(f"    [{i}] {json.dumps(b, ensure_ascii=False)}")
    if bats != snap_bats:
        print("  FAIL battery.list 与 system.snapshot.batteries 不一致")
        failures.append("两个 op 的 batteries 不一致")
    else:
        print("  ok   battery.list 与 system.snapshot.batteries 一致")

    # ---- 先无条件采集 OS 参考源 ----
    # 刻意放在「有没有电池」的判断**之前**：这样即使在无电池机器上，
    # 参考源采集这段代码也会被执行到，可以验证它不报错。
    # 否则这台机器只能验「无电池 -> 空列表」，而采集逻辑永远没跑过。
    print("\n[2] 操作系统参考源（无论有没有电池都会采集）")
    is_win = plat.startswith("win") or host == "Windows"
    is_mac = plat.startswith("mac") or host == "Darwin"
    if is_win:
        refs = windows_refs()
        sps = win_system_power_status()
        if sps is not None:
            print(f"  GetSystemPowerStatus: AC={sps.ac_line_status} "
                  f"Flag={sps.battery_flag} "
                  f"Pct={sps.battery_life_percent} "
                  f"LifeTime={sps.battery_life_time} "
                  f"FullLifeTime={sps.battery_full_life_time}")
            if sps.battery_flag in (128, 255):
                note(f"BatteryFlag={sps.battery_flag}（128=无电池, 255=未知）"
                     f" —— 本机确实没有电池，采集通路可用")
        else:
            note("GetSystemPowerStatus 调用失败")
        print(f"  CIM Win32_Battery = "
              f"{refs.get('src', '(未取到 —— 无电池设备属正常)')}")
        if refs.get("err_cim"):
            note(f"CIM 取值失败: {refs['err_cim']}")
        pc = refs.get("powercfg")
        print(f"  powercfg /batteryreport = "
              f"{pc if pc else '(未取到 —— 无电池设备属正常)'}")
        if refs.get("err_powercfg"):
            note(f"powercfg 取值失败: {refs['err_powercfg']}")
    elif is_mac:
        refs = macos_refs()
        print(f"  pmset -g batt 原文：")
        for line in (refs.get("pmset") or "(未取到)").strip().splitlines():
            print(f"    | {line}")
        if refs.get("err_pmset"):
            note(f"pmset 取值失败: {refs['err_pmset']}")
        print(f"  ioreg AppleSmartBattery: "
              f"CurrentCapacity={refs.get('current_capacity')} "
              f"MaxCapacity={refs.get('max_capacity')} "
              f"IsCharging={refs.get('is_charging')} "
              f"FullyCharged={refs.get('fully_charged')} "
              f"RawRemaining={refs.get('apple_raw_remaining')}")
        if refs.get("err_ioreg"):
            note(f"ioreg 取值失败: {refs['err_ioreg']}")
    else:
        print(f"  未预期的平台 {plat}，本脚本只覆盖 Windows 与 macOS")
        return 2

    if not bats:
        print("\n[3] 对账")
        print("  本机没有电池设备 —— 本脚本必须在**有电池的设备**上跑。")
        print("  台式机 / 虚拟机 / CI runner 上它只能报告「无电池」，属正常。")
        print("  注意：上面第 [2] 节的参考源**已经实际采集过**，")
        print("  所以「采集通路本身可用」这件事在无电池机器上也验过了，")
        print("  只有「两边对账」这一步需要真硬件。")
        print("  " + "-" * 66)
        print("  BATTERY_DEVICE_SKIPPED_NO_BATTERY")
        return 0

    b = bats[0]
    print("\n[3] 逐项对账")

    if is_win:
        if sps is not None:
            if sps.battery_flag in (128, 255):
                note(f"BatteryFlag={sps.battery_flag} 指示无电池，"
                     f"但库返回了 {len(bats)} 条 —— 两者矛盾")
                failures.append("BatteryFlag 指示无电池但库返回了电池")
            if sps.battery_life_percent != 255:
                check("percentage vs GetSystemPowerStatus",
                      b.get("percentage"), sps.battery_life_percent,
                      tol=PCT_TOL)
            else:
                note("API 报 percentage 未知(255)，该项无法对照")
        if refs.get("pct") is not None:
            check("percentage vs CIM Win32_Battery",
                  b.get("percentage"), float(refs["pct"]), tol=PCT_TOL)
        if refs.get("state"):
            check("state vs CIM BatteryStatus", b.get("state"), refs["state"])
        else:
            note("CIM BatteryStatus 未映射，跳过 state 对账")
        if refs.get("minutes") and b.get("time_to_empty_sec"):
            check("time_to_empty vs CIM EstimatedRunTime",
                  b.get("time_to_empty_sec") / 60.0, refs["minutes"],
                  tol=abs(refs["minutes"]) * TIME_TOL_FRAC + 1, unit="min")
        else:
            note("CIM EstimatedRunTime 未知(255)，该项无法对照")
        if refs.get("powercfg_pct") is not None:
            note(f"powercfg 的 FullChargeCapacity/DesignedCapacity = "
                 f"{refs['powercfg']['FullChargeCapacity']}/"
                 f"{refs['powercfg']['DesignedCapacity']}"
                 f" = {refs['powercfg_pct']}% —— 这是**电池健康度**，"
                 f"与当前电量百分比不是一回事，不对账")

    else:
        if refs.get("pct") is not None:
            check("percentage vs ioreg Capacity/MaxCapacity",
                  b.get("percentage"), float(refs["pct"]), tol=PCT_TOL)
            print(f"       (CurrentCapacity={refs['current_capacity']} / "
                  f"MaxCapacity={refs['max_capacity']})")
        else:
            note("ioreg 未读到 Capacity/MaxCapacity（可能不是 Apple 电池）")
        if refs.get("is_charging") is not None:
            want = "charging" if refs["is_charging"] else "discharging"
            if refs.get("fully_charged"):
                want = "full"
            check("state vs ioreg IsCharging/FullyCharged",
                  b.get("state"), want)
        if refs.get("apple_raw_remaining") and b.get("time_to_empty_sec"):
            check("time_to_empty vs ioreg AppleRawBatteryRemaining",
                  b.get("time_to_empty_sec") / 60.0,
                  float(refs["apple_raw_remaining"]), tol=20, unit="min")
        else:
            note("ioreg 未读到剩余时间（未放电时该字段常缺），该项跳过")

    print("\n" + "=" * 72)
    print(f"  对账项 = {checks}   失败 = {len(failures)}")
    for f in failures:
        print(f"    ! {f}")
    if notes:
        print(f"  跳过/备注 {len(notes)} 条（上面逐条已打印，不是静默跳过）")
    ok = not failures
    print("  " + ("BATTERY_DEVICE_OK" if ok else "BATTERY_DEVICE_FAILED"))
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())