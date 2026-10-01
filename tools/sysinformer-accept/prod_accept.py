#!/usr/bin/env python3
"""生产级验收：并发 / 性能 / 泄漏 / 边界负例 / 数值对照。

与"能用"级别的冒烟不同，这里回答的是生产关切：

  1. 边界与负例 —— 非法 op / 非法 pid / 畸形 JSON / 未启动就 poll，
     必须返回**合法信封**（ok:false + 原因）而不是崩溃。
     FFI 里崩溃会带走宿主 JVM，这是生产事故。
  2. 并发安全 —— 多线程并发调用混合 op，不得崩溃、不得返回非法信封。
     原生侧有全局状态（事件队列、缓存），并发是真实风险点。
  3. 性能基线 —— 每个 op 的 p50 / p95 / max 延迟，供容量规划。
  4. 泄漏 —— 反复调用后 RSS 与句柄/fd 数是否增长。
     句柄泄漏在生产里是慢性死亡。
  5. 数值对照 —— 与系统工具交叉核对，证明不是"返回了结构但数字是错的"。

用法：python3 prod_accept.py <libpath> [--platform windows|linux]
"""
import ctypes
import json
import os
import statistics
import subprocess
import sys
import threading
import time

LIB = sys.argv[1]
PLAT = "windows"
for i, a in enumerate(sys.argv):
    if a == "--platform":
        PLAT = sys.argv[i + 1]

lib = ctypes.CDLL(LIB)
lib.sysinformer_call.restype = ctypes.c_void_p
lib.sysinformer_call.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
lib.sysinformer_free_string.argtypes = [ctypes.c_void_p]

passed = 0
failed = 0


def ok(cond, msg):
    global passed, failed
    if cond:
        passed += 1
        print(f"  ASSERT ok   {msg}")
    else:
        failed += 1
        print(f"  ASSERT FAIL {msg}")


def call(op, args=None):
    """返回 (envelope_dict_or_None, raw_str)。None 表示解析失败。"""
    a = json.dumps(args or {}).encode() if not isinstance(args, (bytes, str)) else \
        (args.encode() if isinstance(args, str) else args)
    p = lib.sysinformer_call(op.encode() if isinstance(op, str) else op, a)
    if not p:
        return None, None
    raw = ctypes.cast(p, ctypes.c_char_p).value.decode("utf-8", "replace")
    lib.sysinformer_free_string(p)
    try:
        return json.loads(raw), raw
    except Exception:
        return None, raw


def legal_envelope(env):
    """信封合法性：必须有 ok 字段；ok=false 必须有 error。"""
    if not isinstance(env, dict) or "ok" not in env:
        return False
    if env["ok"] is False and not env.get("error"):
        return False
    return True


def rss_bytes():
    try:
        if PLAT == "linux":
            with open("/proc/self/statm") as f:
                return int(f.read().split()[1]) * os.sysconf("SC_PAGE_SIZE")
        if PLAT == "macos":
            # macOS 的 ru_maxrss 单位是**字节**（Linux 是 KB），不能混用
            import resource
            import sys as _s
            if _s.platform != "darwin":
                return -1
            return int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
        # Windows：必须显式设 argtypes/restype。
        # GetCurrentProcess 的默认返回类型是 32 位 int，在 x64 上会把 64 位句柄
        # **截断**，后续调用全部失败（上一版就因此返回 -1，让泄漏检查空转）。
        class PMC(ctypes.Structure):
            _fields_ = [("cb", ctypes.c_uint32),
                        ("PageFaultCount", ctypes.c_uint32),
                        ("PeakWorkingSetSize", ctypes.c_size_t),
                        ("WorkingSetSize", ctypes.c_size_t),
                        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                        ("QuotaPagedPoolUsage", ctypes.c_size_t),
                        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                        ("PagefileUsage", ctypes.c_size_t),
                        ("PeakPagefileUsage", ctypes.c_size_t)]
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.GetCurrentProcess.restype = ctypes.c_void_p
        psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(PMC),
                                               ctypes.c_uint32]
        psapi.GetProcessMemoryInfo.restype = ctypes.c_int
        pmc = PMC()
        pmc.cb = ctypes.sizeof(PMC)
        if not psapi.GetProcessMemoryInfo(k32.GetCurrentProcess(), ctypes.byref(pmc), pmc.cb):
            return -1
        return pmc.WorkingSetSize
    except Exception:
        return -1


def handle_count():
    try:
        if PLAT in ("linux", "macos"):
            # macOS 也有 /dev/fd（fdescfs），等价于 Linux 的 /proc/self/fd
            if PLAT == "macos":
                return len(os.listdir("/dev/fd"))
            return len(os.listdir("/proc/self/fd"))
        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.GetCurrentProcess.restype = ctypes.c_void_p
        k32.GetProcessHandleCount.argtypes = [ctypes.c_void_p,
                                              ctypes.POINTER(ctypes.c_uint32)]
        k32.GetProcessHandleCount.restype = ctypes.c_int
        n = ctypes.c_uint32(0)
        if not k32.GetProcessHandleCount(k32.GetCurrentProcess(), ctypes.byref(n)):
            return -1
        return n.value
    except Exception:
        return -1


pid = os.getpid()
print(f"  库 = {LIB}")
print(f"  平台 = {PLAT}   本进程 pid = {pid}")
print()

# ============================================================================
print("################ 1) 边界与负例（必须返回合法信封，绝不崩溃）################")
CASES = [
    ("未知 op", b"__no_such_op__", b"{}"),
    ("空 op", b"", b"{}"),
    ("超长 op 名", b"x" * 5000, b"{}"),
    ("畸形 JSON 参数", b"process.detail", b"{not json"),
    ("空参数串", b"process.detail", b""),
    ("缺 pid", b"process.detail", b"{}"),
    ("pid 为负", b"process.detail", b'{"pid":-1}'),
    ("pid 为 0", b"process.detail", b'{"pid":0}'),
    ("pid 超大", b"process.detail", b'{"pid":2147483647}'),
    ("pid 是字符串", b"process.detail", b'{"pid":"abc"}'),
    ("pid 是浮点", b"process.detail", b'{"pid":1.5}'),
    ("未启动就 poll", b"events.poll", b"{}"),
    ("空 mask", b"events.start", b'{"mask":"x"}'),
    ("未知动作", b"action.exec", b'{"kind":"__nope__","target":"1"}'),
    ("动作目标非数字", b"action.exec", b'{"kind":"terminate","target":"abc"}'),
    ("stack 缺 pid", b"process.stack", b"{}"),
    ("stack 非法 tid", b"process.stack", b'{"pid":1,"tid":"x"}'),
    ("socket 非法 pid", b"socket.list", b'{"pid":"x"}'),
]
for name, op, args in CASES:
    env, raw = call(op, args)
    legal = legal_envelope(env)
    ok(legal, f"{name} -> 合法信封（ok={env.get('ok') if env else 'null'}，"
              f"原因={(str(env.get('error'))[:40] if env else str(raw)[:40])}）")
    if env and env.get("ok") is False:
        # 不支持/非法时必须给原因，不能只有 null
        if not env.get("error"):
            ok(False, f"{name} 失败但无 error 字段")

# 负例里 events.start 可能真的把会话建起来了，测完必须收掉，
# 否则后续"事件订阅下并发 poll"会因为"已在运行"而整段跳过（上一版就跳过了）。
call("events.stop")

# 采集函数可用性自检：为 0 会让后面的泄漏检查变成空转
_r0, _h0 = rss_bytes(), handle_count()
ok(_r0 > 0 and _h0 > 0,
   f"本机采集可用（RSS={_r0 / 1e6:.1f}MB，句柄={_h0}）——否则泄漏检查无意义")

# ============================================================================
print()
print("################ 2) 并发安全（多线程并发混合 op）################")
errors = []
results = []


# 采样次数按平台缩放：macOS 上 process.list/detail/tree 各需约 2 秒
# （sysinfo 逐进程 proc_pidinfo 的固有成本，非实现缺陷），
# 用 Windows/Linux 的次数会让该步骤要 22~25 分钟，进而被 concurrency 取消，
# darwin-x86_64 的生产验收就永远跑不完。次数不同但断言强度不变。
IS_MACOS = PLAT == "macos"
CONC_ROUNDS = 6 if IS_MACOS else 15
PERF_ITERS = 10 if IS_MACOS else 30
LEAK_CYCLES = 60 if IS_MACOS else 200
if IS_MACOS:
    print("  [平台调整] macOS 单次 process.list/detail 约 2 秒，"
          "采样次数已缩放（并发 6 / 性能 10 / 泄漏 60），断言条件不变")


def worker(tid):
    try:
        for _ in range(CONC_ROUNDS):
            for op in ("system.snapshot", "process.list", "process.tree",
                       "kernel.modules", "socket.list"):
                env, _ = call(op)
                if not legal_envelope(env):
                    errors.append(f"T{tid} {op} 非法信封")
            env, _ = call("process.detail", {"pid": pid})
            if not legal_envelope(env):
                errors.append(f"T{tid} detail 非法信封")
            results.append(env is not None)
    except Exception as exc:  # 任何异常都算失败（FFI 崩溃会直接带走进程）
        errors.append(f"T{tid} 异常 {type(exc).__name__}: {exc}")


threads = [threading.Thread(target=worker, args=(i,)) for i in range(8)]
t0 = time.time()
for t in threads:
    t.start()
for t in threads:
    t.join()
dt = time.time() - t0
ok(not errors, f"8 线程 × 15 轮 × 6 op 全部返回合法信封（错误 {len(errors)} 条"
               + (f"，样例 {errors[:2]}" if errors else "") + "）")
print(f"      并发耗时 {dt:.2f}s，完成 {len(results)} 次调用")

# 结果一致性：并发后再取一次 process.list，进程数应仍在合理范围
env_a, _ = call("process.list")
env_b, _ = call("process.list")
na = len(env_a.get("data") or []) if env_a and env_a.get("ok") else -1
nb = len(env_b.get("data") or []) if env_b and env_b.get("ok") else -1
ok(na > 0 and nb > 0 and abs(na - nb) < max(50, na // 5),
   f"并发后连续两次 process.list 数量一致（{na} vs {nb}）")

# 事件队列并发：多线程 poll 不应崩
env, _ = call("events.start", {"mask": 65535})
if env and env.get("ok"):
    ev_err = []

    def poller():
        try:
            for _ in range(10):
                e, _ = call("events.poll")
                if not legal_envelope(e):
                    ev_err.append("poll 非法信封")
        except Exception as exc:
            ev_err.append(f"poll 异常 {exc}")

    ps = [threading.Thread(target=poller) for _ in range(4)]
    for t in ps:
        t.start()
    for t in ps:
        t.join()
    ok(not ev_err, f"事件订阅下 4 线程并发 poll 安全（错误 {len(ev_err)}）")
    call("events.stop")
else:
    print(f"      事件订阅不可用（{str(env.get('error'))[:50] if env else 'null'}），跳过并发 poll")

# ============================================================================
print()
print("################ 3) 性能基线（延迟分位，供容量规划）################")
PERF_OPS = [
    ("system.snapshot", {}),
    ("process.list", {}),
    ("process.tree", {}),
    ("kernel.modules", {}),
    ("socket.list", {}),
    ("process.detail", {"pid": pid}),
    ("process.threads", {"pid": pid}),
    ("process.handles", {"pid": pid}),
    ("process.modules", {"pid": pid}),
    ("process.mappings", {"pid": pid}),
    ("process.env", {"pid": pid}),
    ("process.credential", {"pid": pid}),
]
ITERS = PERF_ITERS
print(f"  {'op':<20} {'p50':>9} {'p95':>9} {'max':>9}   (ms, {ITERS} 次)")
for op, args in PERF_OPS:
    lat = []
    failed_here = 0
    for _ in range(ITERS):
        t = time.perf_counter()
        env, _ = call(op, args)
        lat.append((time.perf_counter() - t) * 1000)
        if not legal_envelope(env):
            failed_here += 1
    lat.sort()
    p50 = statistics.median(lat)
    p95 = lat[int(len(lat) * 0.95) - 1]
    mx = lat[-1]
    flag = ""
    if failed_here:
        flag = f"  <== {failed_here} 次非法信封"
        ok(False, f"{op} 全部返回合法信封")
    if p95 > 2000:
        flag += "  <== p95 超 2s"
    print(f"  {op:<20} {p50:>9.2f} {p95:>9.2f} {mx:>9.2f}{flag}")

# process.list 是全量采集，给出规模与单次成本的关系
env, _ = call("process.list")
n = len(env.get("data") or []) if env and env.get("ok") else 0
print(f"      process.list 覆盖 {n} 个进程；若按 1s 周期采样，单次开销即上表 p50")

# ============================================================================
print()
print("################ 4) 泄漏（反复调用后 RSS 与句柄/fd 是否增长）################")
CYCLES = LEAK_CYCLES
r0, h0 = rss_bytes(), handle_count()
for _ in range(CYCLES):
    call("process.list")
    call("system.snapshot")
    call("process.detail", {"pid": pid})
    call("process.modules", {"pid": pid})
r1, h1 = rss_bytes(), handle_count()
print(f"      {CYCLES} 轮 × 4 op：RSS {r0 / 1e6:.1f}MB -> {r1 / 1e6:.1f}MB"
      f"  句柄 {h0} -> {h1}")
if r0 > 0 and r1 > 0:
    grow_mb = (r1 - r0) / 1e6
    # 允许一定波动（分配器缓存），但不应线性增长
    ok(grow_mb < 80, f"RSS 增长 {grow_mb:.1f}MB 在容差内（<80MB）")
if h0 > 0 and h1 > 0:
    ok(h1 - h0 < 100, f"句柄/fd 增长 {h1 - h0} 在容差内（<100）")

# 事件订阅反复启停（容易泄漏 socket / 会话）
leak_before = handle_count()
okc = 0
for _ in range(20):
    e, _ = call("events.start", {"mask": 1})
    if e and e.get("ok"):
        okc += 1
    call("events.stop")
after = handle_count()
if okc > 0:
    print(f"      事件启停 20 轮（成功 {okc} 次）：句柄 {leak_before} -> {after}")
    ok(after - leak_before < 50, f"事件启停未泄漏（增长 {after - leak_before} < 50）")

# ============================================================================
print()
print("################ 5) 数值对照（与系统工具交叉核对）################")
env, _ = call("process.list")
n_api = len(env.get("data") or []) if env and env.get("ok") else 0
if PLAT == "linux":
    out = subprocess.run(["ps", "-e", "--no-headers"], capture_output=True, text=True)
    n_ps = len([l for l in out.stdout.splitlines() if l.strip()])
elif PLAT == "macos":
    # BSD ps 不支持 --no-headers；用 -o pid= 显式要求无表头
    out = subprocess.run(["ps", "-eo", "pid="], capture_output=True, text=True)
    n_ps = len([l for l in out.stdout.splitlines() if l.strip()])
else:
    out = subprocess.run(["tasklist", "/fo", "csv", "/nh"], capture_output=True, text=True,
                         shell=False)
    n_ps = len([l for l in out.stdout.splitlines() if l.strip()])
ok(n_api > 0 and abs(n_api - n_ps) < max(100, n_ps // 4),
   f"进程数对照：api={n_api} vs 系统工具={n_ps}（差异 <25%）")

env, _ = call("system.snapshot")
if env and env.get("ok"):
    d = env["data"]
    cores_api = len(d.get("cpu_cores") or [])
    cores_py = os.cpu_count() or 0
    ok(cores_api == cores_py, f"CPU 逻辑核数对照：api={cores_api} vs os.cpu_count()={cores_py}")

    mem = d.get("memory") or {}
    total_api = mem.get("total") or 0
    if PLAT == "linux":
        with open("/proc/meminfo") as f:
            kb = int([l for l in f if l.startswith("MemTotal")][0].split()[1])
        total_os = kb * 1024
    elif PLAT == "macos":
        out = subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True)
        total_os = int(out.stdout.strip())
    else:
        import ctypes.wintypes as wt
        class MEMSTATUS(ctypes.Structure):
            _fields_ = [("dwLength", wt.DWORD), ("dwMemoryLoad", wt.DWORD),
                        ("ullTotalPhys", ctypes.c_ulonglong),
                        ("ullAvailPhys", ctypes.c_ulonglong),
                        ("ullTotalPageFile", ctypes.c_ulonglong),
                        ("ullAvailPageFile", ctypes.c_ulonglong),
                        ("ullTotalVirtual", ctypes.c_ulonglong),
                        ("ullAvailVirtual", ctypes.c_ulonglong),
                        ("ullAvailExtendedVirtual", ctypes.c_ulonglong)]
        ms = MEMSTATUS()
        ms.dwLength = ctypes.sizeof(MEMSTATUS)
        ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(ms))
        total_os = ms.ullTotalPhys
    diff = abs(total_api - total_os) / max(total_os, 1)
    ok(diff < 0.05, f"内存总量对照：api={total_api / 1e9:.2f}GB vs 系统={total_os / 1e9:.2f}GB"
                    f"（差 {diff * 100:.1f}%）")

    # ---------- A12 电池（2026-10-01 新增，此前无任何 CI 验证）----------
    env_b, _ = call("battery.list")
    ok(env_b is not None and env_b.get("ok") is True,
       f"battery.list 返回合法信封（无电池设备是正常结果，不该是错误）："
       f"ok={env_b.get('ok') if env_b else None}")
    bats_b = (env_b.get("data") if env_b and env_b.get("ok") else None) or []
    bats_s = d.get("batteries") or []
    ok(bats_b == bats_s,
       f"battery.list 与 system.snapshot.batteries 一致"
       f"（{len(bats_b)} vs {len(bats_s)} 条）")

    # 与系统原生接口对照：有电池就逐字段比，没有就确认返回空列表
    if PLAT == "linux":
        # 权威来源是 sysfs；只有 type=Battery 的条目才算电池
        native = []
        pdir = "/sys/class/power_supply"
        if os.path.isdir(pdir):
            for name in sorted(os.listdir(pdir)):
                f = os.path.join(pdir, name, "type")
                try:
                    with open(f) as fh:
                        if fh.read().strip() != "Battery":
                            continue
                except OSError:
                    continue
                entry = {"name": name}
                for key, field in (("capacity", "percentage"),
                                   ("status", "state")):
                    try:
                        with open(os.path.join(pdir, name, key)) as fh:
                            entry[field] = fh.read().strip()
                    except OSError:
                        entry[field] = None
                native.append(entry)
        if native:
            ok(len(bats_b) == len(native),
               f"电池条数对照：api={len(bats_b)} vs sysfs={len(native)}")
            for a, n in zip(bats_b, native):
                cap_n = None
                if n.get("percentage") is not None:
                    try:
                        cap_n = float(n["percentage"])
                    except ValueError:
                        cap_n = None
                cap_a = a.get("percentage")
                ok(cap_a is None or cap_n is None or abs(cap_a - cap_n) <= 2.0,
                   f"电池 {a.get('name')} 电量：api={cap_a}% vs sysfs={cap_n}%")
        else:
            ok(bats_b == [],
               "本机无 /sys/class/power_supply 电池条目 -> api 返回空列表（正确）")
    elif PLAT == "windows":
        import ctypes.wintypes as wt

        class SPS(ctypes.Structure):
            _fields_ = [("ac", ctypes.c_ubyte), ("flag", ctypes.c_ubyte),
                        ("life", ctypes.c_ubyte),
                        ("t1", wt.DWORD), ("t2", wt.DWORD)]

        sps = SPS()
        ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(sps))
        has_batt = sps.flag not in (128, 255)   # 128 = 无系统电池
        if has_batt:
            ok(len(bats_b) > 0,
               f"GetSystemPowerStatus 报告有电池（flag={sps.flag}）-> api 应有 {len(bats_b)} 条")
            if bats_b and sps.life != 255:
                cap_a = bats_b[0].get("percentage")
                ok(cap_a is None or abs(cap_a - float(sps.life)) <= 1.0,
                   f"电量对照：api={cap_a}% vs GetSystemPowerStatus={sps.life}%")
        else:
            ok(bats_b == [],
               f"GetSystemPowerStatus BatteryFlag={sps.flag}（无系统电池）"
               f"-> api 返回空列表（正确）")
    else:  # macOS
        out = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True)
        native_n = len([l for l in out.stdout.splitlines()
                        if l.strip().startswith("-") and "(id=" in l])
        if native_n:
            ok(len(bats_b) == native_n,
               f"电池条数对照：api={len(bats_b)} vs pmset={native_n}")
        else:
            ok(bats_b == [],
               f"pmset 未报告电池（{out.stdout.strip()[:60]}）-> api 返回空列表（正确）")

    host = d.get("host") or {}
    ok(bool(host.get("hostname")), f"hostname = {host.get('hostname')}")

    # 自身进程的 RSS 与 Python 侧对照
    env2, _ = call("process.detail", {"pid": pid})
    if env2 and env2.get("ok"):
        rss_api = env2["data"].get("rss") or 0
        rss_py = rss_bytes()
        if rss_api > 0 and rss_py > 0:
            d2 = abs(rss_api - rss_py) / max(rss_py, 1)
            ok(d2 < 0.5, f"自身 RSS 对照：api={rss_api / 1e6:.1f}MB vs "
                         f"python={rss_py / 1e6:.1f}MB（差 {d2 * 100:.0f}%）")

print()
print(f"  通过 {passed} / 失败 {failed}")
print("PROD_ACCEPT_OK" if failed == 0 else f"PROD_ACCEPT_FAILED failed={failed}")
sys.exit(1 if failed else 0)