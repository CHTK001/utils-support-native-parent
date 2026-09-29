//! Linux 平台实现。
//!
//! # 契约
//! 本文件只需实现一个入口：`call(op, args) -> JSON`，返回值必须是
//! `crate::model::Envelope` 的序列化结果。**不支持的能力必须返回
//! `Envelope { ok: false, error: ... }`**（用 [`unsupported`] 生成），
//! 不要返回空集合——"不支持"与"支持但为空"调用方必须能区分。
//!
//! # 数据来源
//! 进程/内核信息几乎全部来自 `/proc` 与 `/sys`：
//! * `/proc/<pid>/stat` —— 状态、优先级、会话、CPU 时间、启动时刻
//! * `/proc/<pid>/statm` —— 常驻/共享页数
//! * `/proc/<pid>/status` —— Uid/Gid/Groups/CapEff/Seccomp/NoNewPrivs
//! * `/proc/<pid>/io` —— 累计 IO 字节与系统调用次数
//! * `/proc/<pid>/maps` 与 `smaps` —— 内存映射
//! * `/proc/<pid>/fd` 与 `fdinfo` —— 文件描述符
//! * `/proc/<pid>/task/<tid>/*` —— 线程
//! * `/proc/diskstats` / `modules` / `net/*` —— 磁盘、内核模块、套接字
//!
//! # 权限
//! 读取**他进程**的 `environ` / `fd` / `maps` / 内核栈需要**同 uid 或 root**；
//! 本实现遇到权限错误时返回具体路径与提示，不静默返回空集合。
//!
//! # 与 Windows 的语义差异
//! Linux 没有 Windows 的"访问令牌"概念，因此 `CredentialInfo.token_type` /
//! `impersonation_level` / `integrity_level` / `privileges` 一律留 `None`/空，
//! 仅填充 Unix 侧真正存在的 `capabilities` / `seccomp` / `no_new_privs` /
//! `security_label` 等字段。
//!
//! # 本地无法运行验证
//! 本机为 Windows，只能做 `cargo check --target x86_64-unknown-linux-gnu`
//! 类型检查。所有 `/proc` 解析都按"缺文件/缺字段返回 None 或跳过该行"编写，
//! 不假设某个文件一定存在或有第 N 个字段。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use once_cell::sync::Lazy;

use crate::common;
use crate::model::{
    unsupported, ActionKind, ActionResult, CredentialInfo, DiskIo, Envelope, EnvVar, GpuInfo,
    HandleInfo, KernelModuleInfo, MappingInfo, ModuleInfo, ProcessDetail, SensorInfo, ServiceInfo,
    SocketInfo, StackFrame, StackTrace, ThreadInfo,
};
use crate::PLATFORM;

// ============================================================================
// 全局常量（读一次即缓存）
// ============================================================================

/// 每秒的 CPU 时钟节拍数（`sysconf(_SC_CLK_TCK)`），取不到按 100 兜底。
static CLOCK_TICKS: Lazy<u64> = Lazy::new(|| {
    // SAFETY: sysconf 为只读查询，参数为合法常量。
    let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if v > 0 {
        v as u64
    } else {
        100
    }
});

/// 内存页大小（字节），取不到按 4096 兜底。
static PAGE_SIZE: Lazy<u64> = Lazy::new(|| {
    // SAFETY: sysconf 为只读查询，参数为合法常量。
    let v = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if v > 0 {
        v as u64
    } else {
        4096
    }
});

/// 系统启动时刻（Unix 秒），来自 `/proc/stat` 的 `btime`。进程生命周期内不变。
static BOOT_TIME_SEC: Lazy<Option<u64>> = Lazy::new(read_boot_time_sec);

/// Linux capability 名称表，下标即位号（0..=40）。
const CAP_NAMES: [&str; 41] = [
    "CAP_CHOWN",
    "CAP_DAC_OVERRIDE",
    "CAP_DAC_READ_SEARCH",
    "CAP_FOWNER",
    "CAP_FSETID",
    "CAP_KILL",
    "CAP_SETGID",
    "CAP_SETUID",
    "CAP_SETPCAP",
    "CAP_LINUX_IMMUTABLE",
    "CAP_NET_BIND_SERVICE",
    "CAP_NET_BROADCAST",
    "CAP_NET_ADMIN",
    "CAP_NET_RAW",
    "CAP_IPC_LOCK",
    "CAP_IPC_OWNER",
    "CAP_SYS_MODULE",
    "CAP_SYS_RAWIO",
    "CAP_SYS_CHROOT",
    "CAP_SYS_PTRACE",
    "CAP_SYS_PACCT",
    "CAP_SYS_ADMIN",
    "CAP_SYS_BOOT",
    "CAP_SYS_NICE",
    "CAP_SYS_RESOURCE",
    "CAP_SYS_TIME",
    "CAP_SYS_TTY_CONFIG",
    "CAP_MKNOD",
    "CAP_LEASE",
    "CAP_AUDIT_WRITE",
    "CAP_AUDIT_CONTROL",
    "CAP_SETFCAP",
    "CAP_MAC_OVERRIDE",
    "CAP_MAC_ADMIN",
    "CAP_SYSLOG",
    "CAP_WAKE_ALARM",
    "CAP_BLOCK_SUSPEND",
    "CAP_AUDIT_READ",
    "CAP_PERFMON",
    "CAP_BPF",
    "CAP_CHECKPOINT_RESTORE",
];

/// 当前 CPU 时钟节拍数。
///
/// # 返回值
/// 每秒节拍数
fn clock_ticks() -> u64 {
    *CLOCK_TICKS
}

/// 当前内存页大小。
///
/// # 返回值
/// 页字节数
fn page_size() -> u64 {
    *PAGE_SIZE
}

// ============================================================================
// 通用小工具
// ============================================================================

/// 把成功数据包成 `Envelope::ok` 并序列化。
///
/// # 参数
/// * `data` - 要返回的数据
///
/// # 返回值
/// JSON 字符串；序列化理论上不会失败，失败时返回空串
fn ok_json<T: serde::Serialize>(data: T) -> String {
    serde_json::to_string(&Envelope::ok(data)).unwrap_or_default()
}

/// 把错误包成 `Envelope::err` 并序列化。
///
/// # 参数
/// * `e` - 可显示的错误原因
///
/// # 返回值
/// JSON 字符串
fn err_json<E: std::fmt::Display>(e: E) -> String {
    serde_json::to_string(&Envelope::<()>::err(e)).unwrap_or_default()
}

/// 把 `args` 解析成 JSON 值；解析失败按 Null 处理（后续取参一律得到 None）。
///
/// # 参数
/// * `args` - JSON 对象字符串
///
/// # 返回值
/// JSON 值
fn args_value(args: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(args).unwrap_or(serde_json::Value::Null)
}

/// 按 key 取整数值，兼容数字与数字字符串。
///
/// # 参数
/// * `v` - JSON 对象
/// * `key` - 键名
///
/// # 返回值
/// 取到则为整数
fn arg_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    match v.get(key) {
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// 按 key 取 i32。
///
/// # 参数
/// * `v` - JSON 对象
/// * `key` - 键名
///
/// # 返回值
/// 取到则为 i32
fn arg_i32(v: &serde_json::Value, key: &str) -> Option<i32> {
    arg_i64(v, key).map(|n| n as i32)
}

/// 按 key 取布尔值。
///
/// # 参数
/// * `v` - JSON 对象
/// * `key` - 键名
///
/// # 返回值
/// 取到则为布尔
fn arg_bool(v: &serde_json::Value, key: &str) -> Option<bool> {
    match v.get(key) {
        Some(serde_json::Value::Bool(b)) => Some(*b),
        Some(serde_json::Value::Number(n)) => Some(n.as_i64().unwrap_or(0) != 0),
        Some(serde_json::Value::String(s)) => Some(matches!(s.as_str(), "true" | "1" | "yes")),
        _ => None,
    }
}

/// 按 key 取字符串。
///
/// # 参数
/// * `v` - JSON 对象
/// * `key` - 键名
///
/// # 返回值
/// 取到则为字符串
fn arg_str(v: &serde_json::Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
        _ => None,
    }
}

/// 要求 `pid` 参数存在且为正整数。
///
/// # 参数
/// * `a` - JSON 参数对象
///
/// # 返回值
/// 成功为 pid，失败为中文原因
fn need_pid(a: &serde_json::Value) -> Result<i32, String> {
    match arg_i32(a, "pid") {
        Some(p) if p > 0 => Ok(p),
        Some(_) => Err("参数 pid 必须为正整数".to_string()),
        None => Err("缺少参数 pid".to_string()),
    }
}

/// 校验动作目标（pid/tid）有效。
///
/// # 参数
/// * `v` - 目标 ID
///
/// # 返回值
/// 成功为目标 ID，失败为中文原因
fn need_target(v: Option<i32>) -> Result<i32, String> {
    match v {
        Some(p) if p > 0 => Ok(p),
        _ => Err("缺少有效的目标 pid/tid".to_string()),
    }
}

/// 若为权限错误，给出"需同 uid 或 root"的提示后缀。
///
/// # 参数
/// * `e` - IO 错误
///
/// # 返回值
/// 提示后缀，非权限错误时为空串
fn permission_hint(e: &std::io::Error) -> &'static str {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        "（需同 uid 或 root）"
    } else {
        ""
    }
}

/// 读取文本文件，失败时带上路径与错误原因。
///
/// # 参数
/// * `path` - 文件路径
///
/// # 返回值
/// 成功为文件内容，失败为中文原因
fn read_text(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败{}: {}", path, permission_hint(&e), e))
}

/// 读取启动时刻（Unix 秒）。
///
/// # 返回值
/// `/proc/stat` 的 `btime`，取不到为 None
fn read_boot_time_sec() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/stat").ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            if let Ok(v) = rest.trim().parse::<u64>() {
                return Some(v);
            }
        }
    }
    None
}

/// 把时钟节拍换算成毫秒。
///
/// # 参数
/// * `ticks` - 节拍数
///
/// # 返回值
/// 毫秒
fn ticks_to_ms(ticks: u64) -> u64 {
    ticks.saturating_mul(1000) / clock_ticks().max(1)
}

/// 读链接目标为字符串。
///
/// # 参数
/// * `path` - 链接路径
///
/// # 返回值
/// 目标字符串，读不到为 None
fn read_link_str(path: &str) -> Option<String> {
    std::fs::read_link(path)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// 解析"起始-结束"十六进制地址区间。
///
/// # 参数
/// * `s` - 形如 `7f00-8000` 的串
///
/// # 返回值
/// (起始, 结束) 字节地址
fn parse_range(s: Option<&str>) -> Option<(u64, u64)> {
    let s = s?;
    let (a, b) = s.split_once('-')?;
    Some((u64::from_str_radix(a, 16).ok()?, u64::from_str_radix(b, 16).ok()?))
}

/// 解析可选的 `FromStr` 值。
///
/// # 参数
/// * `s` - 可选字符串
///
/// # 返回值
/// 解析成功则为值
fn parse_opt<T: std::str::FromStr>(s: Option<&str>) -> Option<T> {
    s.and_then(|x| x.parse::<T>().ok())
}

/// 取 `/proc` 中的 kB 数值并换算成字节。
///
/// # 参数
/// * `val` - 形如 `1234 kB` 的值串
///
/// # 返回值
/// 字节数
fn parse_kb(val: &str) -> Option<u64> {
    let digits: String = val.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u64>().ok().map(|v| v.saturating_mul(1024))
}

/// 进程状态字符转可读名。
///
/// # 参数
/// * `c` - `/proc/<pid>/stat` 的状态字符
///
/// # 返回值
/// 可读状态名
fn state_name(c: char) -> String {
    match c {
        'R' => "running",
        'S' => "sleeping",
        'D' => "disk-sleep",
        'Z' => "zombie",
        'T' => "stopped",
        't' => "tracing-stop",
        'I' => "idle",
        'X' | 'x' => "dead",
        'K' => "wakekill",
        'W' => "waking",
        'P' => "parked",
        _ => return c.to_string(),
    }
    .to_string()
}

/// 调度策略号转可读类别。
///
/// # 参数
/// * `policy` - `/proc/<pid>/stat` 的 policy 字段
///
/// # 返回值
/// normal / batch / idle / realtime / deadline / unknown
fn policy_class_name(policy: u32) -> String {
    match policy {
        0 => "normal",
        1 | 2 => "realtime",
        3 => "batch",
        5 => "idle",
        6 => "deadline",
        _ => "unknown",
    }
    .to_string()
}

// ============================================================================
// /proc/<pid>/stat 解析
// ============================================================================

/// `/proc/<pid>/stat` 中本实现用到的字段。
///
/// 字段位次基于内核文档 `proc(5)`：`comm` 之后（含括号后第一个空格）第 0 个为
/// `state`。因为 `comm` 本身可能含空格与括号，必须先定位**最后一个** `)`。
struct ProcStat {
    /// 括号内的进程名。
    comm: Option<String>,
    /// 状态字符。
    state: Option<char>,
    /// 父进程 ID。
    ppid: Option<i32>,
    /// 会话 ID。
    session: Option<u32>,
    /// 用户态 CPU 节拍。
    utime: Option<u64>,
    /// 内核态 CPU 节拍。
    stime: Option<u64>,
    /// 内核调度优先级。
    priority: Option<i32>,
    /// nice 值。
    nice: Option<i32>,
    /// 线程数。
    num_threads: Option<u32>,
    /// 启动时刻（自开机起的节拍）。
    starttime: Option<u64>,
    /// 虚拟内存字节。
    vsize: Option<u64>,
    /// 常驻内存页数。
    rss_pages: Option<u64>,
    /// 代码段起始地址。
    startcode: Option<u64>,
    /// 初始栈地址。
    startstack: Option<u64>,
    /// 实时优先级。
    rt_priority: Option<i32>,
    /// 调度策略。
    policy: Option<u32>,
}

/// 解析 `/proc/<pid>/stat` 内容，缺字段一律返回 None。
///
/// # 参数
/// * `content` - 文件全文
///
/// # 返回值
/// 解析结果；连 `)` 都找不到则为 None
fn parse_stat(content: &str) -> Option<ProcStat> {
    let close = content.rfind(')')?;
    let comm = content
        .find('(')
        .and_then(|open| content.get(open + 1..close))
        .map(|s| s.to_string());
    let rest = content.get(close + 1..).unwrap_or("");
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let p = |i: usize| -> Option<&str> { parts.get(i).copied() };
    let pi = |i: usize| -> Option<i32> { p(i).and_then(|s| s.parse::<i32>().ok()) };
    let pu = |i: usize| -> Option<u64> { p(i).and_then(|s| s.parse::<u64>().ok()) };
    Some(ProcStat {
        comm,
        state: p(0).and_then(|s| s.chars().next()),
        ppid: pi(1),
        session: p(3).and_then(|s| s.parse::<u32>().ok()),
        utime: pu(11),
        stime: pu(12),
        priority: pi(15),
        nice: pi(16),
        num_threads: p(17).and_then(|s| s.parse::<u32>().ok()),
        starttime: pu(19),
        vsize: pu(20),
        rss_pages: pu(21),
        startcode: pu(23),
        startstack: pu(25),
        rt_priority: pi(37),
        policy: p(38).and_then(|s| s.parse::<u32>().ok()),
    })
}

/// 读取并解析指定 stat 文件。
///
/// # 参数
/// * `path` - stat 文件路径
///
/// # 返回值
/// 解析结果
fn read_stat_path(path: &str) -> Option<ProcStat> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_stat(&content)
}

/// 读取 `/proc/<pid>/stat`。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 解析结果
fn read_stat(pid: i32) -> Option<ProcStat> {
    read_stat_path(&format!("/proc/{}/stat", pid))
}

/// 取进程的有效优先级：实时进程用 rt_priority，普通进程用 nice。
///
/// # 参数
/// * `st` - stat 解析结果
///
/// # 返回值
/// 优先级数值
fn effective_priority(st: &ProcStat) -> Option<i32> {
    match st.rt_priority {
        Some(rt) if rt > 0 => Some(rt),
        _ => st.nice.or(st.priority),
    }
}

/// 由自开机节拍换算成 Unix 毫秒启动时刻。
///
/// # 参数
/// * `starttime_ticks` - `/proc/<pid>/stat` 的 starttime
///
/// # 返回值
/// Unix 毫秒；缺少 btime 时为 None
fn start_time_ms(starttime_ticks: u64) -> Option<i64> {
    let btime = (*BOOT_TIME_SEC)?;
    Some((btime as i64 + (starttime_ticks / clock_ticks().max(1)) as i64) * 1000)
}

// ============================================================================
// /proc/<pid>/status、statm、io 解析
// ============================================================================

/// `/proc/<pid>/status` 中本实现用到的字段。
///
/// 大量字段目前仅用于 credential，其余场景不读取，故整体允许 dead_code。
#[allow(dead_code)]
#[derive(Default)]
struct ProcStatus {
    /// Name 字段。
    name: Option<String>,
    /// State 字段首字符。
    state: Option<char>,
    /// 真实 UID。
    uid_real: Option<u32>,
    /// 有效 UID。
    uid_eff: Option<u32>,
    /// 保存的 UID。
    uid_saved: Option<u32>,
    /// 文件系统 UID。
    uid_fs: Option<u32>,
    /// 真实 GID。
    gid_real: Option<u32>,
    /// 有效 GID。
    gid_eff: Option<u32>,
    /// 附加组 ID 列表。
    groups: Vec<String>,
    /// 有效 capability 掩码。
    cap_eff: Option<u64>,
    /// seccomp 模式。
    seccomp: Option<u32>,
    /// no_new_privs 标记。
    no_new_privs: Option<u32>,
    /// 线程数。
    threads: Option<u32>,
    /// 常驻内存。
    vm_rss: Option<u64>,
    /// 虚拟内存。
    vm_size: Option<u64>,
    /// 数据段。
    vm_data: Option<u64>,
    /// swap 用量。
    vm_swap: Option<u64>,
}

/// 解析 `/proc/<pid>/status`。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 解析结果；文件不可读时为 None
fn parse_status(pid: i32) -> Option<ProcStatus> {
    let path = format!("/proc/{}/status", pid);
    let content = std::fs::read_to_string(&path).ok()?;
    let mut s = ProcStatus::default();
    for line in content.lines() {
        let (key, val) = match line.split_once(':') {
            Some(kv) => kv,
            None => {
                continue;
            }
        };
        let key = key.trim();
        let val = val.trim();
        match key {
            "Name" => {
                s.name = Some(val.to_string());
            }
            "State" => {
                s.state = val.chars().next();
            }
            "Uid" => {
                let v: Vec<&str> = val.split_whitespace().collect();
                s.uid_real = parse_opt(v.first().copied());
                s.uid_eff = parse_opt(v.get(1).copied());
                s.uid_saved = parse_opt(v.get(2).copied());
                s.uid_fs = parse_opt(v.get(3).copied());
            }
            "Gid" => {
                let v: Vec<&str> = val.split_whitespace().collect();
                s.gid_real = parse_opt(v.first().copied());
                s.gid_eff = parse_opt(v.get(1).copied());
            }
            "Groups" => {
                s.groups = val.split_whitespace().map(|x| x.to_string()).collect();
            }
            "CapEff" => {
                s.cap_eff = u64::from_str_radix(val, 16).ok();
            }
            "Seccomp" => {
                s.seccomp = val.parse::<u32>().ok();
            }
            "NoNewPrivs" => {
                s.no_new_privs = val.parse::<u32>().ok();
            }
            "Threads" => {
                s.threads = val.parse::<u32>().ok();
            }
            "VmRSS" => {
                s.vm_rss = parse_kb(val);
            }
            "VmSize" => {
                s.vm_size = parse_kb(val);
            }
            "VmData" => {
                s.vm_data = parse_kb(val);
            }
            "VmSwap" => {
                s.vm_swap = parse_kb(val);
            }
            _ => {}
        }
    }
    Some(s)
}

/// 读取 `/proc/<pid>/statm` 的（共享字节, 常驻字节）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// (shared_bytes, resident_bytes)
fn read_statm(pid: i32) -> Option<(u64, u64)> {
    let path = format!("/proc/{}/statm", pid);
    let content = std::fs::read_to_string(&path).ok()?;
    let parts: Vec<&str> = content.split_whitespace().collect();
    let resident = parts.get(1)?.parse::<u64>().ok()?;
    let shared = parts.get(2)?.parse::<u64>().ok()?;
    let ps = page_size();
    Some((shared.saturating_mul(ps), resident.saturating_mul(ps)))
}

/// `/proc/<pid>/io` 中本实现用到的字段。
struct ProcIo {
    /// 累计读字节。
    read_bytes: u64,
    /// 累计写字节。
    write_bytes: u64,
    /// 读系统调用次数。
    read_count: u64,
    /// 写系统调用次数。
    write_count: u64,
}

/// 读取 `/proc/<pid>/io`。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// IO 累计值；无权限或文件不存在时为 None
fn read_io(pid: i32) -> Option<ProcIo> {
    let path = format!("/proc/{}/io", pid);
    let content = std::fs::read_to_string(&path).ok()?;
    let mut io = ProcIo { read_bytes: 0, write_bytes: 0, read_count: 0, write_count: 0 };
    for line in content.lines() {
        let (k, v) = match line.split_once(':') {
            Some(kv) => kv,
            None => {
                continue;
            }
        };
        let val = match v.trim().parse::<u64>() {
            Ok(x) => x,
            Err(_) => {
                continue;
            }
        };
        match k.trim() {
            "read_bytes" => {
                io.read_bytes = val;
            }
            "write_bytes" => {
                io.write_bytes = val;
            }
            "syscr" => {
                io.read_count = val;
            }
            "syscw" => {
                io.write_count = val;
            }
            _ => {}
        }
    }
    Some(io)
}

/// 统计 `/proc/<pid>/fd` 下的 fd 数。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// fd 数；无权限时为 None
fn count_fds(pid: i32) -> Option<u32> {
    let dir = format!("/proc/{}/fd", pid);
    let rd = std::fs::read_dir(&dir).ok()?;
    Some(rd.filter_map(|e| e.ok()).count() as u32)
}

/// 读取 `/proc/<pid>/cmdline`（NUL 分隔）并拆成参数数组。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 参数列表；内核线程等无 cmdline 时为空
fn read_cmdline(pid: i32) -> Vec<String> {
    match std::fs::read(format!("/proc/{}/cmdline", pid)) {
        Ok(bytes) => bytes
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ============================================================================
// 进程详情 / 列表
// ============================================================================

/// 用 `/proc` 数据补齐 `ProcessDetail` 的 Linux 专属字段。
///
/// 只覆盖"能从 /proc 拿到"的字段；`cpu_usage` / `rss` 等 sysinfo 更准的值
/// 由调用方在之前或之后合并，本函数仅在原值为空/0 时填写。
///
/// # 参数
/// * `p` - 待补齐的进程详情
fn fill_linux_fields(p: &mut ProcessDetail) {
    let pid = p.pid;
    if let Some(st) = read_stat(pid) {
        if p.ppid.is_none() {
            p.ppid = st.ppid;
        }
        if let Some(sid) = st.session {
            p.session_id = Some(sid);
        }
        p.priority = effective_priority(&st);
        p.priority_class = st.policy.map(policy_class_name);
        if p.thread_count == 0 {
            if let Some(n) = st.num_threads {
                p.thread_count = n;
            }
        }
        if p.start_time_ms.is_none() {
            p.start_time_ms = st.starttime.and_then(start_time_ms);
        }
        if p.virtual_memory == 0 {
            if let Some(v) = st.vsize {
                p.virtual_memory = v;
            }
        }
        if p.rss == 0 {
            if let Some(rp) = st.rss_pages {
                p.rss = rp.saturating_mul(page_size());
            }
        }
    }
    if let Some((shared, resident)) = read_statm(pid) {
        p.shared_bytes = Some(shared);
        p.private_bytes = Some(resident.saturating_sub(shared));
    }
    if let Some(io) = read_io(pid) {
        p.io_read_bytes = Some(io.read_bytes);
        p.io_written_bytes = Some(io.write_bytes);
        p.io_read_count = Some(io.read_count);
        p.io_write_count = Some(io.write_count);
    }
    p.handle_count = count_fds(pid);
    if p.is_elevated.is_none() {
        if let Some(s) = parse_status(pid) {
            p.is_elevated = Some(s.uid_eff == Some(0));
        }
    }
}

/// 完全从 `/proc` 构造一个进程详情（不依赖 sysinfo）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为详情；进程不存在或 stat 不可读为中文原因
fn proc_detail(pid: i32) -> Result<ProcessDetail, String> {
    let stat = read_stat(pid)
        .ok_or_else(|| format!("读取 /proc/{}/stat 失败：进程不存在或无权限", pid))?;
    let status = parse_status(pid);
    let uid_eff = status.as_ref().and_then(|s| s.uid_eff);
    let gid_eff = status.as_ref().and_then(|s| s.gid_eff);
    let name = std::fs::read_to_string(format!("/proc/{}/comm", pid))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| stat.comm.clone())
        .or_else(|| status.as_ref().and_then(|s| s.name.clone()))
        .unwrap_or_default();
    let args = read_cmdline(pid);
    let start_ms = stat.starttime.and_then(start_time_ms);
    let run_time_sec = start_ms.and_then(|s| {
        let now = common::now_millis();
        if now >= s {
            Some(((now - s) / 1000) as u64)
        } else {
            None
        }
    });
    let mut d = ProcessDetail {
        pid,
        ppid: stat.ppid,
        name,
        session_id: stat.session,
        user: uid_eff.and_then(common::user_name),
        uid: uid_eff.map(|u| u as i32),
        gid: gid_eff.map(|g| g as i32),
        status: stat.state.map(state_name).unwrap_or_else(|| "unknown".to_string()),
        priority: effective_priority(&stat),
        priority_class: stat.policy.map(policy_class_name),
        start_time_ms: start_ms,
        run_time_sec,
        // Linux 无 WOW64 概念，留 None 表示"未采集/不适用"。
        is_wow64: None,
        is_elevated: uid_eff.map(|u| u == 0),
        is_protected: None,
        cpu_usage: 0.0,
        rss: stat.rss_pages.map(|p| p.saturating_mul(page_size())).unwrap_or(0),
        virtual_memory: stat.vsize.unwrap_or(0),
        private_bytes: None,
        shared_bytes: None,
        thread_count: stat.num_threads.unwrap_or(0),
        handle_count: None,
        io_read_bytes: None,
        io_written_bytes: None,
        io_read_count: None,
        io_write_count: None,
        command_line: if args.is_empty() {
            None
        } else {
            Some(args.join(" "))
        },
        args,
        exe_path: read_link_str(&format!("/proc/{}/exe", pid)),
        cwd: read_link_str(&format!("/proc/{}/cwd", pid)),
        root_dir: read_link_str(&format!("/proc/{}/root", pid)),
        signature: None,
    };
    fill_linux_fields(&mut d);
    Ok(d)
}

// ============================================================================
// 线程 / 环境变量 / 模块 / 映射 / 句柄 / 凭据
// ============================================================================

/// 遍历 `/proc/<pid>/task` 收集线程信息。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为线程列表；task 目录不可读为中文原因
fn threads_of(pid: i32) -> Result<Vec<ThreadInfo>, String> {
    let dir = format!("/proc/{}/task", pid);
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| format!("读取 {} 失败{}: {}", dir, permission_hint(&e), e))?;
    let mut out = Vec::new();
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                continue;
            }
        };
        let tid = match entry.file_name().to_string_lossy().parse::<i64>() {
            Ok(t) => t,
            Err(_) => {
                continue;
            }
        };
        let stat_path = format!("/proc/{}/task/{}/stat", pid, tid);
        let st = match read_stat_path(&stat_path) {
            Some(s) => s,
            None => {
                continue;
            }
        };
        let comm = std::fs::read_to_string(format!("/proc/{}/task/{}/comm", pid, tid))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let wait_reason = std::fs::read_to_string(format!("/proc/{}/task/{}/wchan", pid, tid))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "0");
        out.push(ThreadInfo {
            tid,
            pid,
            status: st.state.map(state_name),
            priority: effective_priority(&st),
            user_time_ms: st.utime.map(ticks_to_ms),
            kernel_time_ms: st.stime.map(ticks_to_ms),
            start_address: st.startcode.map(|v| format!("0x{:x}", v)),
            stack_base: st.startstack.map(|v| format!("0x{:x}", v)),
            wait_reason,
            name: comm,
        });
    }
    Ok(out)
}

/// 读取 `/proc/<pid>/environ`（NUL 分隔的 `K=V`）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为变量列表（可为空）；权限不足为带"需同 uid 或 root"的中文原因
fn env_of(pid: i32) -> Result<Vec<EnvVar>, String> {
    let path = format!("/proc/{}/environ", pid);
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("读取 {} 失败{}: {}", path, permission_hint(&e), e))?;
    let mut out = Vec::new();
    for seg in bytes.split(|b| *b == 0) {
        if seg.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(seg);
        match s.split_once('=') {
            Some((k, v)) => out.push(EnvVar { key: k.to_string(), value: v.to_string() }),
            None => out.push(EnvVar { key: s.into_owned(), value: String::new() }),
        }
    }
    Ok(out)
}

/// 从 `/proc/<pid>/maps` 提取 `.so` 映射为已加载模块。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为模块列表；maps 不可读为中文原因
fn modules_of(pid: i32) -> Result<Vec<ModuleInfo>, String> {
    let path = format!("/proc/{}/maps", pid);
    let content = read_text(&path)?;
    let mut map: HashMap<String, (u64, u64)> = HashMap::new();
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let range = it.next();
        let _perms = it.next();
        let _offset = it.next();
        let _dev = it.next();
        let _inode = it.next();
        let file: String = it.collect::<Vec<_>>().join(" ");
        if file.is_empty() {
            continue;
        }
        if !file.ends_with(".so") && !file.contains(".so.") {
            continue;
        }
        let (start, end) = match parse_range(range) {
            Some(v) => v,
            None => {
                continue;
            }
        };
        let e = map.entry(file).or_insert((start, end));
        if start < e.0 {
            e.0 = start;
        }
        if end > e.1 {
            e.1 = end;
        }
    }
    let mut out = Vec::new();
    for (path, (start, end)) in map {
        let name = path.rsplit('/').next().unwrap_or(path.as_str()).to_string();
        out.push(ModuleInfo {
            name,
            path: Some(path),
            base_address: Some(format!("0x{:x}", start)),
            size: Some(end.saturating_sub(start)),
            version: None,
            company: None,
            description: None,
            signature: None,
        });
    }
    Ok(out)
}

/// 按映射文件与权限推断映射类型。
///
/// # 参数
/// * `path` - 映射文件路径（可为空）
/// * `perms` - 权限串，如 `r-xp`
///
/// # 返回值
/// image / mapped / private / stack / heap
fn classify_mapping(path: &str, perms: &str) -> String {
    if path.starts_with("[stack") {
        return "stack".to_string();
    }
    if path.starts_with("[heap") {
        return "heap".to_string();
    }
    if path.is_empty() {
        return "private".to_string();
    }
    if path.starts_with('[') {
        return "private".to_string();
    }
    if perms.contains('x') {
        return "image".to_string();
    }
    "mapped".to_string()
}

/// 解析与 maps 同格式的内容（maps 或 smaps 的头部行）。
///
/// # 参数
/// * `content` - 文件全文
///
/// # 返回值
/// 映射列表
fn parse_maps_like(content: &str) -> Vec<MappingInfo> {
    let mut out = Vec::new();
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let range = it.next();
        let (start, end) = match parse_range(range) {
            Some(v) => v,
            None => {
                continue;
            }
        };
        let perms = it.next().unwrap_or("");
        let _offset = it.next();
        let _dev = it.next();
        let _inode = it.next();
        let file: String = it.collect::<Vec<_>>().join(" ");
        out.push(MappingInfo {
            base_address: format!("0x{:x}", start),
            size: end.saturating_sub(start),
            protection: perms.chars().take(3).collect(),
            kind: classify_mapping(&file, perms),
            path: if file.is_empty() { None } else { Some(file) },
        });
    }
    out
}

/// 读取进程内存映射，优先用 smaps 拿到更准的类型信息。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为映射列表；maps 不可读为中文原因
fn mappings_of(pid: i32) -> Result<Vec<MappingInfo>, String> {
    let smaps = format!("/proc/{}/smaps", pid);
    if let Ok(content) = std::fs::read_to_string(&smaps) {
        let parsed = parse_maps_like(&content);
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }
    let maps = format!("/proc/{}/maps", pid);
    let content = read_text(&maps)?;
    Ok(parse_maps_like(&content))
}

/// 读取 `/proc/<pid>/fdinfo/<fd>` 中指定字段。
///
/// # 参数
/// * `pid` - 进程 ID
/// * `fd` - 文件描述符号
/// * `key` - 字段名（冒号前）
///
/// # 返回值
/// 字段值
fn read_fdinfo_field(pid: i32, fd: &str, key: &str) -> Option<String> {
    let path = format!("/proc/{}/fdinfo/{}", pid, fd);
    let content = std::fs::read_to_string(&path).ok()?;
    for line in content.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// 遍历 `/proc/<pid>/fd` 收集句柄信息。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为句柄列表；fd 目录不可读为中文原因
fn handles_of(pid: i32) -> Result<Vec<HandleInfo>, String> {
    let dir = format!("/proc/{}/fd", pid);
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| format!("读取 {} 失败{}: {}", dir, permission_hint(&e), e))?;
    let mut out = Vec::new();
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                continue;
            }
        };
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.parse::<u64>().is_err() {
            continue;
        }
        let link_path = format!("{}/{}", dir, id);
        let target = std::fs::read_link(&link_path)
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        let (kind, name) = match &target {
            Some(t) if t.starts_with("socket:") => ("socket".to_string(), Some(t.clone())),
            Some(t) if t.starts_with("pipe:") => ("pipe".to_string(), Some(t.clone())),
            Some(t) if t.starts_with("anon_inode:") => ("event".to_string(), Some(t.clone())),
            Some(t) => {
                let is_dir = std::fs::metadata(&link_path)
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                let kind = if is_dir { "directory" } else { "file" };
                (kind.to_string(), Some(t.clone()))
            }
            None => {
                // 链接不可读时用 fdinfo 的 inode 判断是否为套接字。
                let kind = match read_fdinfo_field(pid, &id, "ino") {
                    Some(_) => "socket".to_string(),
                    None => "unknown".to_string(),
                };
                (kind, None)
            }
        };
        let access = read_fdinfo_field(pid, &id, "flags");
        out.push(HandleInfo { id, kind, name, access, ref_count: None });
    }
    Ok(out)
}

/// seccomp 模式号转可读名。
///
/// # 参数
/// * `mode` - `/proc/<pid>/status` 的 Seccomp 值
///
/// # 返回值
/// disabled / strict / filter / unknown
fn seccomp_name(mode: u32) -> String {
    match mode {
        0 => "disabled",
        1 => "strict",
        2 => "filter",
        _ => "unknown",
    }
    .to_string()
}

/// 把 capability 掩码解成名字列表。
///
/// # 参数
/// * `mask` - CapEff 掩码
///
/// # 返回值
/// 已置位的 capability 名称
fn decode_caps(mask: u64) -> Vec<String> {
    let mut out = Vec::new();
    for (i, name) in CAP_NAMES.iter().enumerate() {
        if i < 64 && (mask & (1u64 << i)) != 0 {
            out.push((*name).to_string());
        }
    }
    out
}

/// 读取 AppArmor / SELinux 标签。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 标签字符串；文件不存在或为空时为 None
fn read_security_label(pid: i32) -> Option<String> {
    let path = format!("/proc/{}/attr/current", pid);
    let s = std::fs::read_to_string(&path).ok()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 读取进程凭据信息。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 成功为凭据；status 不可读为中文原因
fn credential_of(pid: i32) -> Result<CredentialInfo, String> {
    let status = parse_status(pid)
        .ok_or_else(|| format!("读取 /proc/{}/status 失败：进程不存在或无权限", pid))?;
    Ok(CredentialInfo {
        owner: status.uid_eff.and_then(common::user_name),
        // 以下四项是 Windows "访问令牌"专有语义，Linux 无对应概念，留 None。
        token_type: None,
        impersonation_level: None,
        integrity_level: None,
        elevated: status.uid_eff.map(|u| u == 0),
        groups: status.groups.clone(),
        // privileges 为 Windows 特权，Linux 用 capabilities 表达。
        privileges: Vec::new(),
        capabilities: status.cap_eff.map(decode_caps).unwrap_or_default(),
        seccomp: status.seccomp.map(seccomp_name),
        no_new_privs: status.no_new_privs.map(|v| v != 0),
        security_label: read_security_label(pid),
        // entitlements 为 macOS 概念。
        entitlements: Vec::new(),
    })
}

// ============================================================================
// 磁盘 IO / 内核模块 / 服务
// ============================================================================

/// 读取 `/proc/diskstats`。
///
/// # 返回值
/// 成功为磁盘 IO 列表；文件不可读为中文原因
fn disk_io() -> Result<Vec<DiskIo>, String> {
    let content = read_text("/proc/diskstats")?;
    let mut out = Vec::new();
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 12 {
            continue;
        }
        // 字段位次：0 主号,1 次号,2 设备名,3 读完成,4 读合并,5 读扇区,
        // 6 读耗时,7 写完成,8 写合并,9 写扇区,10 写耗时,11 进行中。
        let read_count = f[3].parse::<u64>().unwrap_or(0);
        let sectors_read = f[5].parse::<u64>().unwrap_or(0);
        let write_count = f[7].parse::<u64>().unwrap_or(0);
        let sectors_written = f[9].parse::<u64>().unwrap_or(0);
        let queue_depth = f[11].parse::<u64>().ok();
        out.push(DiskIo {
            name: f[2].to_string(),
            read_bytes: sectors_read.saturating_mul(512),
            written_bytes: sectors_written.saturating_mul(512),
            read_count,
            write_count,
            queue_depth,
        });
    }
    Ok(out)
}

/// 读取 `/proc/modules`。
///
/// # 返回值
/// 成功为内核模块列表；文件不可读为中文原因
fn kernel_modules() -> Result<Vec<KernelModuleInfo>, String> {
    let content = read_text("/proc/modules")?;
    let mut out = Vec::new();
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        let size = f.get(1).and_then(|s| s.parse::<u64>().ok());
        // 末列通常是 `0x...` 基址；不满足则留 None。
        let base = f
            .last()
            .filter(|s| s.starts_with("0x"))
            .map(|s| (*s).to_string());
        out.push(KernelModuleInfo {
            name: f[0].to_string(),
            // 模块名无法可靠映射到 .ko 文件路径（依赖内核版本与模块别名），留 None。
            path: None,
            base_address: base,
            size,
        });
    }
    Ok(out)
}

/// 执行一次 systemctl 子命令并返回 stdout。
///
/// # 参数
/// * `args` - 传给 systemctl 的参数
///
/// # 返回值
/// 成功为 stdout；调用失败或退出码非 0 为中文原因
fn run_systemctl(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| format!("调用 systemctl 失败: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "systemctl {:?} 退出码 {:?}: {}",
            args,
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 列出 systemd 服务；非 systemd 环境返回不支持。
///
/// # 返回值
/// 成功为服务列表；非 systemd 或 systemctl 调用失败为中文原因
fn services() -> Result<Vec<ServiceInfo>, String> {
    if !Path::new("/run/systemd/system").exists() {
        return Err(unsupported("service.list（当前不是 systemd 环境，未找到 /run/systemd/system）", PLATFORM));
    }
    let unit_files = run_systemctl(&[
        "list-unit-files",
        "--type=service",
        "--no-pager",
        "--no-legend",
        "--plain",
    ])?;
    let units = run_systemctl(&[
        "list-units",
        "--type=service",
        "--all",
        "--no-pager",
        "--no-legend",
        "--plain",
    ])?;
    let mut map: HashMap<String, ServiceInfo> = HashMap::new();
    for line in unit_files.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 2 || !f[0].ends_with(".service") {
            continue;
        }
        let start_type = match f[1] {
            "enabled" | "enabled-runtime" => "auto",
            "disabled" | "masked" | "masked-runtime" => "disabled",
            "static" | "alias" | "indirect" | "generated" | "transient" => "manual",
            _ => "unknown",
        };
        let unit = f[0].to_string();
        map.insert(
            unit.clone(),
            ServiceInfo {
                name: unit.clone(),
                display_name: unit,
                state: "unknown".to_string(),
                start_type: start_type.to_string(),
                account: None,
                binary_path: None,
                is_driver: None,
                pid: None,
            },
        );
    }
    for line in units.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 || !f[0].ends_with(".service") {
            continue;
        }
        let active = f[2];
        let state = if active == "active" {
            "running"
        } else if active == "inactive" || active == "failed" {
            "stopped"
        } else {
            "unknown"
        };
        let desc = f[4..].join(" ");
        let unit = f[0].to_string();
        let entry = map.entry(unit.clone()).or_insert_with(|| ServiceInfo {
            name: unit.clone(),
            display_name: unit,
            state: "unknown".to_string(),
            start_type: "unknown".to_string(),
            account: None,
            binary_path: None,
            is_driver: None,
            pid: None,
        });
        entry.state = state.to_string();
        if !desc.is_empty() {
            entry.display_name = desc;
        }
    }
    let mut out: Vec<ServiceInfo> = map.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

// ============================================================================
// 套接字
// ============================================================================

/// 解析 `socket:[inode]` 形式的链接目标。
///
/// # 参数
/// * `target` - 链接目标字符串
///
/// # 返回值
/// socket inode
fn parse_socket_inode(target: &str) -> Option<u64> {
    let inner = target.strip_prefix("socket:[")?.strip_suffix(']')?;
    inner.parse::<u64>().ok()
}

/// 把 `/proc/net/*` 的 IPv4 十六进制地址（小端）转成点分十进制。
///
/// # 参数
/// * `hex` - 8 位十六进制串
///
/// # 返回值
/// IPv4 文本
fn decode_v4(hex: &str) -> Option<String> {
    if hex.len() != 8 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    let b = n.to_le_bytes();
    Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]))
}

/// 把 `/proc/net/*` 的 IPv6 十六进制地址（4 个小端 32 位字）转成标准文本。
///
/// # 参数
/// * `hex` - 32 位十六进制串
///
/// # 返回值
/// IPv6 文本
fn decode_v6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for i in 0..4 {
        let s = hex.get(i * 8..i * 8 + 8)?;
        let w = u32::from_str_radix(s, 16).ok()?;
        bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    Some(std::net::Ipv6Addr::from(bytes).to_string())
}

/// 解析 `地址:端口` 字段。
///
/// # 参数
/// * `field` - `/proc/net/*` 的地址列
/// * `is_v6` - 是否 IPv6
///
/// # 返回值
/// `ip:port` 文本
fn decode_addr(field: &str, is_v6: bool) -> Option<String> {
    let (addr_hex, port_hex) = field.split_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    let ip = if is_v6 {
        decode_v6(addr_hex)?
    } else {
        decode_v4(addr_hex)?
    };
    Some(format!("{}:{}", ip, port))
}

/// 判断地址列是否全零（未连接/监听）。
///
/// # 参数
/// * `field` - 地址:端口 列
///
/// # 返回值
/// 地址部分是否全为 0
fn is_zero_addr(field: &str) -> bool {
    match field.split_once(':') {
        Some((addr, _)) => addr.chars().all(|c| c == '0'),
        None => true,
    }
}

/// 十六进制 TCP 状态码转可读名。
///
/// # 参数
/// * `code` - st 列
/// * `proto` - 协议名
///
/// # 返回值
/// 状态名
fn tcp_state_name(code: &str, proto: &str) -> String {
    let c = code.to_ascii_uppercase();
    let name = match c.as_str() {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        "0C" => "NEW_SYN_RECV",
        _ => return format!("0x{}", c),
    };
    if proto.starts_with("udp") && c == "07" {
        return "UNCONNECTED".to_string();
    }
    name.to_string()
}

/// 扫描 `/proc/<pid>/fd` 得到该进程持有的 socket inode 集合。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// inode 集合
fn inodes_of_pid(pid: i32) -> HashSet<u64> {
    let mut set = HashSet::new();
    let dir = format!("/proc/{}/fd", pid);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Ok(t) = std::fs::read_link(e.path()) {
                if let Some(ino) = parse_socket_inode(&t.to_string_lossy()) {
                    set.insert(ino);
                }
            }
        }
    }
    set
}

/// 扫描所有进程建立 socket inode -> pid 映射（取第一个命中的 pid）。
///
/// # 返回值
/// inode 到 pid 的映射
fn build_inode_owner() -> HashMap<u64, i32> {
    let mut map = HashMap::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let pid = match e.file_name().to_string_lossy().parse::<i32>() {
                Ok(p) => p,
                Err(_) => {
                    continue;
                }
            };
            let dir = format!("/proc/{}/fd", pid);
            if let Ok(fds) = std::fs::read_dir(&dir) {
                for fd in fds.flatten() {
                    if let Ok(t) = std::fs::read_link(fd.path()) {
                        if let Some(ino) = parse_socket_inode(&t.to_string_lossy()) {
                            map.entry(ino).or_insert(pid);
                        }
                    }
                }
            }
        }
    }
    map
}

/// 列出 TCP/UDP 套接字，可选按 pid 过滤。
///
/// # 参数
/// * `a` - JSON 参数对象，`pid` 可选
///
/// # 返回值
/// 成功为套接字列表；读取 `/proc/net/*` 全失败为中文原因
fn socket_list(a: &serde_json::Value) -> Result<Vec<SocketInfo>, String> {
    let pid_filter = arg_i32(a, "pid");
    let owner_inodes = match pid_filter {
        Some(p) => inodes_of_pid(p),
        None => HashSet::new(),
    };
    let inode_owner = if pid_filter.is_none() {
        build_inode_owner()
    } else {
        HashMap::new()
    };
    let mut out = Vec::new();
    let mut any_file = false;
    let sources: [(&str, &str); 4] = [
        ("/proc/net/tcp", "tcp"),
        ("/proc/net/tcp6", "tcp6"),
        ("/proc/net/udp", "udp"),
        ("/proc/net/udp6", "udp6"),
    ];
    for (path, proto) in sources {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                continue;
            }
        };
        any_file = true;
        let is_v6 = proto.ends_with('6');
        for line in content.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let inode = f[9].parse::<u64>().ok();
            let pid = match pid_filter {
                Some(p) => match inode {
                    Some(ino) if owner_inodes.contains(&ino) => Some(p),
                    _ => {
                        continue;
                    }
                },
                None => inode.and_then(|i| inode_owner.get(&i).copied()),
            };
            let local = match decode_addr(f[1], is_v6) {
                Some(v) => v,
                None => {
                    continue;
                }
            };
            let state = tcp_state_name(f[3], proto);
            let remote = if f[3].eq_ignore_ascii_case("0A") || is_zero_addr(f[2]) {
                None
            } else {
                decode_addr(f[2], is_v6)
            };
            out.push(SocketInfo {
                protocol: proto.to_string(),
                local,
                remote,
                state,
                pid,
                inode,
            });
        }
    }
    if !any_file {
        return Err("读取 /proc/net/tcp|tcp6|udp|udp6 全部失败".to_string());
    }
    Ok(out)
}

// ============================================================================
// 动作执行
// ============================================================================

/// 发送信号并构造动作结果。
///
/// # 参数
/// * `kind` - 动作类型
/// * `target` - 目标 pid/tid
/// * `sig` - 信号编号
///
/// # 返回值
/// 动作结果
fn kill_action(kind: ActionKind, target: i32, sig: i32) -> ActionResult {
    // SAFETY: kill 是标准 POSIX 调用，参数为目标 pid 与信号号。
    let r = unsafe { libc::kill(target as libc::pid_t, sig) };
    if r == 0 {
        ActionResult { kind, target: target.to_string(), ok: true, error: None }
    } else {
        let e = std::io::Error::last_os_error();
        ActionResult {
            kind,
            target: target.to_string(),
            ok: false,
            error: Some(format!("kill({}, {}) 失败: {}", target, sig, e)),
        }
    }
}

/// 设置进程 nice 值并构造动作结果。
///
/// # 参数
/// * `pid` - 目标进程
/// * `nice` - nice 值
///
/// # 返回值
/// 动作结果
fn priority_action(pid: i32, nice: i32) -> ActionResult {
    // SAFETY: setpriority 为只读该进程优先级的系统调用。
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
    if r == 0 {
        ActionResult {
            kind: ActionKind::SetPriority,
            target: pid.to_string(),
            ok: true,
            error: None,
        }
    } else {
        let e = std::io::Error::last_os_error();
        ActionResult {
            kind: ActionKind::SetPriority,
            target: pid.to_string(),
            ok: false,
            error: Some(format!("setpriority({}, {}) 失败: {}", pid, nice, e)),
        }
    }
}

/// 从参数解析 CPU 亲和性掩码，支持数组、位掩码数字、逗号分隔串。
///
/// # 参数
/// * `a` - JSON 参数对象
///
/// # 返回值
/// CPU 序号列表
fn arg_cpus(a: &serde_json::Value) -> Vec<usize> {
    match a.get("mask") {
        Some(serde_json::Value::Array(arr)) => {
            arr.iter().filter_map(|v| v.as_u64().map(|x| x as usize)).collect()
        }
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .map(|m| (0..64).filter(|i| (m >> i) & 1 == 1).map(|i| i as usize).collect())
            .unwrap_or_default(),
        Some(serde_json::Value::String(s)) => {
            s.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).collect()
        }
        _ => Vec::new(),
    }
}

/// 设置进程 CPU 亲和性并构造动作结果。
///
/// # 参数
/// * `pid` - 目标进程
/// * `cpus` - CPU 序号列表
///
/// # 返回值
/// 动作结果
fn affinity_action(pid: i32, cpus: &[usize]) -> ActionResult {
    let fail = |msg: String| ActionResult {
        kind: ActionKind::SetAffinity,
        target: pid.to_string(),
        ok: false,
        error: Some(msg),
    };
    if cpus.is_empty() {
        return fail("mask 为空，无法设置亲和性".to_string());
    }
    let max_cpu = cpus.iter().copied().max().unwrap_or(0);
    let mut buf = vec![0u8; (max_cpu / 8 + 1).max(8)];
    for &c in cpus {
        buf[c / 8] |= 1u8 << (c % 8);
    }
    // SAFETY: buf 长度覆盖所有待设置的 CPU 位，ptr 在调用期间有效。
    let r = unsafe {
        libc::sched_setaffinity(
            pid as libc::pid_t,
            buf.len(),
            buf.as_ptr() as *const libc::cpu_set_t,
        )
    };
    if r == 0 {
        ActionResult {
            kind: ActionKind::SetAffinity,
            target: pid.to_string(),
            ok: true,
            error: None,
        }
    } else {
        let e = std::io::Error::last_os_error();
        fail(format!("sched_setaffinity({}, {:?}) 失败: {}", pid, cpus, e))
    }
}

/// 执行一个动作。
///
/// # 参数
/// * `a` - JSON 参数对象，含 `kind`、`pid`/`tid`/`target`、`priority`、`mask`
///
/// # 返回值
/// 成功（含动作本身失败）为 `ActionResult`；未知动作或不支持为中文原因
fn action_exec(a: &serde_json::Value) -> Result<ActionResult, String> {
    let raw_kind = arg_str(a, "kind").unwrap_or_default();
    let key = raw_kind
        .to_ascii_lowercase()
        .replace('_', "")
        .replace('-', "")
        .replace(' ', "");
    let target_pid = arg_i32(a, "pid").or_else(|| arg_i32(a, "target"));
    let target_tid = arg_i32(a, "tid");
    match key.as_str() {
        "terminate" | "terminateprocess" => {
            let pid = need_target(target_pid)?;
            Ok(kill_action(ActionKind::TerminateProcess, pid, libc::SIGKILL))
        }
        "suspend" | "suspendprocess" => {
            let pid = need_target(target_pid)?;
            Ok(kill_action(ActionKind::SuspendProcess, pid, libc::SIGSTOP))
        }
        "resume" | "resumeprocess" => {
            let pid = need_target(target_pid)?;
            Ok(kill_action(ActionKind::ResumeProcess, pid, libc::SIGCONT))
        }
        "suspendthread" => {
            let tid = need_target(target_tid.or(target_pid))?;
            Ok(kill_action(ActionKind::SuspendThread, tid, libc::SIGSTOP))
        }
        "resumethread" => {
            let tid = need_target(target_tid.or(target_pid))?;
            Ok(kill_action(ActionKind::ResumeThread, tid, libc::SIGCONT))
        }
        "setpriority" => {
            let pid = need_target(target_pid)?;
            let nice = arg_i32(a, "priority").unwrap_or(0);
            Ok(priority_action(pid, nice))
        }
        "setaffinity" => {
            let pid = need_target(target_pid)?;
            let cpus = arg_cpus(a);
            Ok(affinity_action(pid, &cpus))
        }
        "closehandle" => Err(unsupported(
            "action.exec closehandle（Linux 无法关闭他进程的 fd）",
            PLATFORM,
        )),
        "" => Err("缺少参数 kind".to_string()),
        other => Err(unsupported(&format!("action.exec 动作 {}", other), PLATFORM)),
    }
}

// ============================================================================
// 栈回溯
// ============================================================================

/// 解析 `/proc/<pid>/task/<tid>/stack` 的内核栈文本。
///
/// 典型每行：`[<ffffffff810a1b30>] __schedule+0x2e0/0x7c0`
///
/// # 参数
/// * `content` - 文件全文
///
/// # 返回值
/// 栈帧列表
fn parse_kernel_stack(content: &str) -> Vec<StackFrame> {
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let (addr_raw, rest) = match (t.find('['), t.find(']')) {
            (Some(a), Some(b)) if b > a => (&t[a + 1..b], t[b + 1..].trim()),
            _ => ("", t),
        };
        let addr = addr_raw.trim_matches(|c| c == '<' || c == '>').trim();
        let token = match rest.split_whitespace().next() {
            Some(x) if !x.is_empty() => x,
            _ => {
                continue;
            }
        };
        let (symbol, offset) = match token.split_once('+') {
            Some((s, o)) => (s.to_string(), Some(o.to_string())),
            None => (token.to_string(), None),
        };
        out.push(StackFrame {
            address: if addr.is_empty() { "unknown".to_string() } else { addr.to_string() },
            module: None,
            module_offset: offset,
            symbol: Some(symbol),
        });
    }
    out
}

/// 取栈回溯。内核栈读 `/proc/<pid>/task/<tid>/stack`；用户态栈未实现。
///
/// # 参数
/// * `a` - JSON 参数对象，含 `pid`、可选 `tid`、可选 `kernel`
///
/// # 返回值
/// 成功为 `StackTrace`（读取失败原因写在 `error` 字段）；用户态栈或不支持的请求为中文原因
fn stack_trace(a: &serde_json::Value) -> Result<StackTrace, String> {
    let pid = need_pid(a)?;
    let tid = arg_i32(a, "tid").unwrap_or(pid);
    let kernel = arg_bool(a, "kernel").unwrap_or(true);
    if !kernel {
        return Err(unsupported(
            "process.stack 用户态栈回溯（需 libunwind，本实现未实现）",
            PLATFORM,
        ));
    }
    let path = format!("/proc/{}/task/{}/stack", pid, tid);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let frames = parse_kernel_stack(&content);
            let error = if frames.is_empty() {
                Some(format!(
                    "{} 为空或内容受限（读取内核栈通常需 root / CAP_SYS_ADMIN）",
                    path
                ))
            } else {
                None
            };
            Ok(StackTrace { pid, tid: Some(tid as i64), kernel: true, frames, error })
        }
        Err(e) => Ok(StackTrace {
            pid,
            tid: Some(tid as i64),
            kernel: true,
            frames: Vec::new(),
            error: Some(format!("读取 {} 失败{}: {}", path, permission_hint(&e), e)),
        }),
    }
}

// ============================================================================
// GPU / 传感器
// ============================================================================

/// 判断 DRM 条目名是否为 `cardN`（排除 `cardN-DP-1` 这类连接器）。
///
/// # 参数
/// * `name` - `/sys/class/drm` 下的条目名
///
/// # 返回值
/// 是否为主显卡节点
fn is_card_name(name: &str) -> bool {
    match name.strip_prefix("card") {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// 从 `/sys/.../device/uevent` 读取内核驱动名。
///
/// # 参数
/// * `dev` - 设备目录
///
/// # 返回值
/// 驱动名，取不到为空串
fn read_driver(dev: &Path) -> String {
    match std::fs::read_to_string(dev.join("uevent")) {
        Ok(content) => {
            for line in content.lines() {
                if let Some(v) = line.strip_prefix("DRIVER=") {
                    return v.trim().to_string();
                }
            }
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// 读取整数文件。
///
/// # 参数
/// * `p` - 文件路径
///
/// # 返回值
/// 解析出的整数
fn read_u64_file(p: &Path) -> Option<u64> {
    std::fs::read_to_string(p).ok()?.trim().parse::<u64>().ok()
}

/// 读取浮点文件。
///
/// # 参数
/// * `p` - 文件路径
///
/// # 返回值
/// 解析出的浮点数
fn read_f32_file(p: &Path) -> Option<f32> {
    std::fs::read_to_string(p).ok()?.trim().parse::<f32>().ok()
}

/// 枚举 GPU。
///
/// NVIDIA 从 `/proc/driver/nvidia/gpus/*/information` 取型号（无需 NVML）；
/// Intel/AMD 从 `/sys/class/drm/card*/device` 取厂商与驱动，AMD 另可读
/// `gpu_busy_percent` / `mem_info_vram_*`。取不到的字段一律留 None。
///
/// # 返回值
/// GPU 列表；无可识别设备时为空
fn gpus() -> Vec<GpuInfo> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/proc/driver/nvidia/gpus") {
        for e in rd.flatten() {
            let info = e.path().join("information");
            if let Ok(content) = std::fs::read_to_string(&info) {
                let mut model = String::new();
                for line in content.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("Model") {
                            model = v.trim().to_string();
                        }
                    }
                }
                if !model.is_empty() {
                    out.push(GpuInfo {
                        vendor: "nvidia".to_string(),
                        name: model,
                        memory_total: None,
                        memory_used: None,
                        usage: None,
                        temperature_c: None,
                        power_w: None,
                        driver_version: None,
                    });
                }
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
        for e in rd.flatten() {
            let card = e.file_name().to_string_lossy().into_owned();
            if !is_card_name(&card) {
                continue;
            }
            let dev = e.path().join("device");
            if !dev.exists() {
                continue;
            }
            let vendor_id = std::fs::read_to_string(dev.join("vendor"))
                .ok()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let vendor = match vendor_id.as_str() {
                "0x8086" => "intel",
                "0x1002" => "amd",
                "0x10de" => "nvidia",
                _ => "unknown",
            };
            // NVIDIA 已在上面的 /proc 分支处理，这里跳过以免重复。
            if vendor == "nvidia" {
                continue;
            }
            let driver = read_driver(&dev);
            let name = if driver.is_empty() {
                card.clone()
            } else {
                format!("{} ({})", card, driver)
            };
            out.push(GpuInfo {
                vendor: vendor.to_string(),
                name,
                memory_total: read_u64_file(&dev.join("mem_info_vram_total")),
                memory_used: read_u64_file(&dev.join("mem_info_vram_used")),
                usage: read_f32_file(&dev.join("gpu_busy_percent")),
                temperature_c: None,
                power_w: None,
                driver_version: if driver.is_empty() { None } else { Some(driver) },
            });
        }
    }
    out
}

/// 收集某个 hwmon 芯片下某一类传感器。
///
/// # 参数
/// * `dir` - hwmon 目录
/// * `chip` - 芯片名
/// * `prefix` - 文件前缀，如 `temp`
/// * `kind` - 传感器类别
/// * `unit` - 单位
/// * `divisor` - 原始值除数（毫度/毫伏需除 1000）
/// * `out` - 输出列表
fn collect_sensors(
    dir: &Path,
    chip: &str,
    prefix: &str,
    kind: &str,
    unit: &str,
    divisor: f32,
    out: &mut Vec<SensorInfo>,
) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => {
            return;
        }
    };
    for e in rd.flatten() {
        let fname = e.file_name().to_string_lossy().into_owned();
        let rest = match fname.strip_prefix(prefix) {
            Some(r) => r,
            None => {
                continue;
            }
        };
        let idx = match rest.strip_suffix("_input") {
            Some(i) => i,
            None => {
                continue;
            }
        };
        if idx.is_empty() || !idx.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let raw = match std::fs::read_to_string(e.path()) {
            Ok(s) => s,
            Err(_) => {
                continue;
            }
        };
        let value = match raw.trim().parse::<f32>() {
            Ok(v) => v,
            Err(_) => {
                continue;
            }
        };
        let label = std::fs::read_to_string(dir.join(format!("{}{}_label", prefix, idx)))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let name = match label {
            Some(l) => format!("{} {}", chip, l),
            None => format!("{} {}{}", chip, prefix, idx),
        };
        out.push(SensorInfo {
            name,
            kind: kind.to_string(),
            value: value / divisor,
            unit: unit.to_string(),
        });
    }
}

/// 枚举 hwmon 传感器（温度/风扇/电压）。
///
/// # 返回值
/// 传感器列表；无 `/sys/class/hwmon` 时为空
fn sensors() -> Vec<SensorInfo> {
    let mut out = Vec::new();
    let base = Path::new("/sys/class/hwmon");
    let rd = match std::fs::read_dir(base) {
        Ok(r) => r,
        Err(_) => {
            return out;
        }
    };
    for e in rd.flatten() {
        let dir = e.path();
        let chip = std::fs::read_to_string(dir.join("name"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
        // 温度：毫摄氏度；电压：毫伏；风扇：RPM。
        collect_sensors(&dir, &chip, "temp", "temperature", "°C", 1000.0, &mut out);
        collect_sensors(&dir, &chip, "fan", "fan", "RPM", 1.0, &mut out);
        collect_sensors(&dir, &chip, "in", "voltage", "V", 1000.0, &mut out);
    }
    out
}

// ============================================================================
// 入口
// ============================================================================

/// 平台入口：按 op 分发。
///
/// # 参数
/// * `op` - 操作名
/// * `args` - JSON 参数，无参时为空串
///
/// # 返回值
/// 序列化后的 `Envelope`
pub fn call(op: &str, args: &str) -> String {
    let a = args_value(args);
    match op {
        // ---------- 进程列表 / 树 ----------
        "process.list" => {
            let mut list = common::sysinfo_processes();
            for p in list.iter_mut() {
                fill_linux_fields(p);
            }
            ok_json(list)
        }
        "process.tree" => ok_json(common::process_tree()),

        // ---------- 进程详情 ----------
        "process.detail" => match need_pid(&a) {
            Ok(pid) => match proc_detail(pid) {
                Ok(mut d) => {
                    // 合并 sysinfo 的 CPU/内存等跨平台指标（Linux /proc 拿不到使用率差值）。
                    if let Some(s) = common::sysinfo_processes().into_iter().find(|p| p.pid == pid) {
                        d.cpu_usage = s.cpu_usage;
                        if s.rss > 0 {
                            d.rss = s.rss;
                        }
                        if s.virtual_memory > 0 {
                            d.virtual_memory = s.virtual_memory;
                        }
                        if d.user.is_none() {
                            d.user = s.user;
                        }
                        if d.command_line.is_none() {
                            d.command_line = s.command_line;
                        }
                        if d.args.is_empty() {
                            d.args = s.args;
                        }
                        if d.exe_path.is_none() {
                            d.exe_path = s.exe_path;
                        }
                        if d.cwd.is_none() {
                            d.cwd = s.cwd;
                        }
                        if d.root_dir.is_none() {
                            d.root_dir = s.root_dir;
                        }
                        if d.run_time_sec.is_none() {
                            d.run_time_sec = s.run_time_sec;
                        }
                        if d.thread_count == 0 {
                            d.thread_count = s.thread_count;
                        }
                    }
                    ok_json(d)
                }
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },

        // ---------- 线程 / 环境变量 ----------
        "process.threads" => match need_pid(&a) {
            Ok(pid) => match threads_of(pid) {
                Ok(v) => ok_json(v),
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },
        "process.env" => match need_pid(&a) {
            Ok(pid) => match env_of(pid) {
                Ok(v) => ok_json(v),
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },

        // ---------- 模块 / 映射 / 句柄 / 凭据 ----------
        "process.modules" => match need_pid(&a) {
            Ok(pid) => match modules_of(pid) {
                Ok(v) => ok_json(v),
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },
        "process.mappings" => match need_pid(&a) {
            Ok(pid) => match mappings_of(pid) {
                Ok(v) => ok_json(v),
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },
        "process.handles" => match need_pid(&a) {
            Ok(pid) => match handles_of(pid) {
                Ok(v) => ok_json(v),
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },
        "process.credential" => match need_pid(&a) {
            Ok(pid) => match credential_of(pid) {
                Ok(v) => ok_json(v),
                Err(e) => err_json(e),
            },
            Err(e) => err_json(e),
        },

        // ---------- 栈回溯 ----------
        "process.stack" => match stack_trace(&a) {
            Ok(v) => ok_json(v),
            Err(e) => err_json(e),
        },

        // ---------- 系统级 ----------
        "disk.io" => match disk_io() {
            Ok(v) => ok_json(v),
            Err(e) => err_json(e),
        },
        "kernel.modules" => match kernel_modules() {
            Ok(v) => ok_json(v),
            Err(e) => err_json(e),
        },
        "service.list" => match services() {
            Ok(v) => ok_json(v),
            Err(e) => err_json(e),
        },
        "socket.list" => match socket_list(&a) {
            Ok(v) => ok_json(v),
            Err(e) => err_json(e),
        },

        // ---------- 动作 ----------
        "action.exec" => match action_exec(&a) {
            Ok(v) => ok_json(v),
            Err(e) => err_json(e),
        },

        // ---------- 可选能力 ----------
        "gpu.list" => ok_json(gpus()),
        "sensor.list" => ok_json(sensors()),
        "memory.modules" => err_json(unsupported(
            "memory.modules（需 root 且依赖 dmidecode 解析 DMI，本实现未做）",
            PLATFORM,
        )),
        "events.start" | "events.poll" | "events.stop" => err_json(unsupported(
            "进程事件驱动（netlink proc connector CN_IDX_PROC，需 root 或 CAP_NET_ADMIN，本实现未实现）",
            PLATFORM,
        )),

        other => err_json(unsupported(other, PLATFORM)),
    }
}
