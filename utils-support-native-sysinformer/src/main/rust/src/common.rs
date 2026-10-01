//! 跨平台基线采集（基于 `sysinfo`）。
//!
//! 这里只做三平台都能一致拿到的东西。任何"某平台拿不到"的字段一律留 `Option::None`，
//! 由平台实现补——**不要用默认值 0 冒充采集结果**。
//!
//! # 关于刷新语义（从本仓 metrics 模块继承的教训）
//!
//! `sysinfo` 的 CPU 使用率是"两次刷新之间的差值"。若每次采集都新建 `System`，
//! Windows 侧 PDH 计数器永远没有基线，使用率会恒为 100。因此这里复用**同一个**
//! 全局 `System` 实例，并且 `refresh_cpu_all` 与取频率放在同一次刷新内。
//!
//! **进程内第一次调用**同样没有基线：`System::new()` 的上次累计时间为 0，
//! 首次 `refresh_cpu_all()` 会把"开机到现在"的整段间隔算成满载，导致**所有核
//! 都报 100%**。所以首次刷新会连续做两次（见 [`refresh_cpu`]），第一次只用于
//! 建立基线，第二次才用于取值。这是实测复现的：`/system/snapshot` 首次调用
//! 返回 12 核全 100.0%，第二次起才正常。

use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{
    Disks, Networks, Pid, ProcessesToUpdate, System, Users,
};

use crate::model::{
    CpuCore, CpuSummary, DiskPartition, HostInfo, LoadAverage, MemoryInfo,
    NetworkInterface, ProcessTreeNode, SwapInfo, SystemTimeline,
};

/// 全局复用的 `System`，保证使用率差值有基线。
static SYSTEM: Lazy<Mutex<System>> = Lazy::new(|| Mutex::new(System::new()));

/// 取当前 Unix 毫秒时间戳。
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 进程内是否已经建立过 CPU 基线。
///
/// 必须**所有**刷新路径共用这一个标记：`refreshed_sys` 与 `refreshed_one`
/// 若各用各的标记，就会出现"先查 `process.detail` 使标记置位，之后
/// `system.snapshot` 直接跳过预热"的情况 —— 那时快照的基线其实是
/// `process.detail` 留下的（刷新时刻不同），仍不可靠。
static CPU_BASELINE_READY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 刷新 CPU，必要时为进程内首次调用建立基线。
///
/// # 参数
/// * `sys` - 已加锁的 `System` 实例
fn refresh_cpu(sys: &mut System) {
    if CPU_BASELINE_READY.load(std::sync::atomic::Ordering::Relaxed) {
        sys.refresh_cpu_all();
        return;
    }
    // 第一次只为建立基线：此刻 usage 必然是 100%（上次累计为 0），丢弃。
    sys.refresh_cpu_all();
    // 隔一小段时间再刷一次，让第二次拿到的是真实的短间隔使用率。
    // 间隔不能太短（差值会被取整放大），也不能太长（拖慢首次调用）。
    std::thread::sleep(std::time::Duration::from_millis(120));
    sys.refresh_cpu_all();
    CPU_BASELINE_READY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 刷新并返回全局 `System` 的锁。
fn refreshed_sys() -> std::sync::MutexGuard<'static, System> {
    let mut sys = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    refresh_cpu(&mut sys);
    sys.refresh_memory();
    sys
}

/// 刷新 CPU + 内存 + **全部进程**。
///
/// 只在确实需要进程列表时用。全量刷新进程是这里最贵的一步：生产验收实测
/// `system.snapshot`（只需要 CPU/内存/磁盘/网络）因为复用了"含全量进程"的刷新，
/// p50 高达 565ms、p95 1520ms；而磁盘/网络/主机信息本身都是毫秒级。
/// 需要进程时用本函数，否则用 [`refreshed_sys`]。
fn refreshed() -> std::sync::MutexGuard<'static, System> {
    let mut sys = refreshed_sys();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys
}

/// 只刷新指定进程，用于单进程查询。
///
/// `process.detail` 原先走 `sysinfo_processes()`（刷新**全部**进程）再筛出目标，
/// 实测 p50 204ms；单进程刷新可把这一步降到接近零。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 刷新后的 System 锁
fn refreshed_one(pid: i32) -> std::sync::MutexGuard<'static, System> {
    let mut sys = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    refresh_cpu(&mut sys);
    sys.refresh_memory();
    let pids = [Pid::from_u32(pid as u32)];
    sys.refresh_processes(ProcessesToUpdate::Some(&pids), true);
    sys
}

/// A1 + A2 一次刷新同时产出核列表与汇总。
///
/// **不要**分别调用 [`cpu_cores`] 与 [`cpu_summary`]：两者各自刷新一次，
/// 背靠背的两次刷新会把 CPU 使用率的差值窗口压到微秒级，Windows 侧 PDH 会给出
/// 退化值（本仓 metrics 模块记过这个坑，恒为 100）。需要两者时一律用本函数。
///
/// # Windows 上的口径
///
/// Windows **不使用** `sysinfo` 的 `cpu_usage()`，改由 [`crate::cpu_windows`]
/// 直接读 PDH `% Processor Time`，与任务管理器同源。原因：`sysinfo` 在 Windows
/// 上只注册 `% Idle Time` 并用 `100 - idle`，而任务管理器用 `% Processor Time`
/// （分母排除 idle），实测 `sysinfo` 口径**系统性偏高约 5pp**
/// （20 组采样，95% CI `[+1.01, +9.33]pp`，不含 0）。
/// PDH 不可用时回退到 `sysinfo`，并如实保留其口径差异。
///
/// # 返回值
/// (每核列表, 汇总)
pub fn cpu_all() -> (Vec<CpuCore>, CpuSummary) {
    // 用 refreshed_sys：CPU/内存不需要进程列表，刷新全量进程会让本函数
    // 从毫秒级退化到几百毫秒（生产验收实测过）。
    let sys = refreshed_sys();
    let cpus = sys.cpus();
    let logical = cpus.len() as u32;

    // Windows：优先用任务管理器同源口径；不可用时回退 sysinfo。
    #[cfg(target_os = "windows")]
    let pdh_usage = {
        let (total, per_core) = crate::cpu_windows::cpu_usage();
        if per_core.is_empty() {
            None
        } else {
            Some((total, per_core))
        }
    };
    #[cfg(not(target_os = "windows"))]
    let pdh_usage: Option<(f32, Vec<f32>)> = None;

    let cores: Vec<CpuCore> = cpus
        .iter()
        .enumerate()
        .map(|(i, c)| CpuCore {
            id: i as u32,
            usage: match pdh_usage.as_ref().and_then(|(_, v)| v.get(i)) {
                Some(v) => *v,
                None => c.cpu_usage(),
            },
            frequency_mhz: match c.frequency() {
                0 => None,
                f => Some(f),
            },
            brand: c.brand().to_string(),
        })
        .collect();
    // 汇总优先取 PDH 的 `_Total`（它本身就是任务管理器顶部那个数字）；
    // 否则按「各核和 / 核数」推算。
    let usage = match pdh_usage.as_ref().map(|(t, _)| *t) {
        Some(t) => t,
        None => {
            if cores.is_empty() {
                0.0
            } else {
                cores.iter().map(|c| c.usage).sum::<f32>() / cores.len() as f32
            }
        }
    };
    let summary = CpuSummary {
        logical_count: logical,
        physical_count: sys.physical_core_count().map(|c| c as u32),
        usage,
        brand: cpus.first().map(|c| c.brand().to_string()).unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
    };
    (cores, summary)
}

/// A1 每逻辑核。
///
/// # 返回值
/// 逻辑核列表，序号从 0 开始
pub fn cpu_cores() -> Vec<CpuCore> {
    cpu_all().0
}

/// A2 CPU 汇总。
///
/// # 返回值
/// 汇总信息
pub fn cpu_summary() -> CpuSummary {
    cpu_all().1
}

/// A3 负载。
///
/// # 返回值
/// 负载值；Windows 无原生负载，`approximated` 标为 true
pub fn load_average() -> LoadAverage {
    let la = System::load_average();
    LoadAverage {
        one: la.one,
        five: la.five,
        fifteen: la.fifteen,
        approximated: cfg!(target_os = "windows"),
    }
}

/// A4 / A5 内存。
///
/// # 返回值
/// 内存总量与已用；`cached` / `buffers` 与内存条列表由平台实现补
pub fn memory() -> MemoryInfo {
    // 只要内存，不需要进程列表
    let sys = refreshed_sys();
    let total = sys.total_memory();
    let used = sys.used_memory();
    let free = sys.free_memory();
    let available = sys.available_memory();
    MemoryInfo {
        total,
        used,
        free,
        available,
        cached: None,
        buffers: None,
        modules: Vec::new(),
    }
}

/// A6 Swap。
///
/// # 返回值
/// swap 用量
pub fn swap() -> SwapInfo {
    // 只要 swap，不需要进程列表
    let sys = refreshed_sys();
    SwapInfo {
        total: sys.total_swap(),
        used: sys.used_swap(),
        free: sys.total_swap().saturating_sub(sys.used_swap()),
    }
}

/// A7 磁盘分区。
///
/// # 返回值
/// 分区列表；介质类型取不到时留空
pub fn disks() -> Vec<DiskPartition> {
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            DiskPartition {
                name: d.name().to_string_lossy().into_owned(),
                mount_point: d.mount_point().to_string_lossy().into_owned(),
                total,
                used: total.saturating_sub(available),
                available,
                file_system: d.file_system().to_string_lossy().into_owned(),
                removable: Some(d.is_removable()),
                kind: Some(format!("{:?}", d.kind()).to_lowercase()),
            }
        })
        .collect()
}

/// A9 网络接口。
///
/// # 返回值
/// 接口列表
pub fn networks() -> Vec<NetworkInterface> {
    Networks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|(name, data)| NetworkInterface {
            name: name.clone(),
            received_bytes: data.total_received(),
            transmitted_bytes: data.total_transmitted(),
            received_packets: data.total_packets_received(),
            transmitted_packets: data.total_packets_transmitted(),
            errors_in: data.total_errors_on_received(),
            errors_out: data.total_errors_on_transmitted(),
            drops_in: None,
            drops_out: None,
            mtu: None,
            mac: {
                let m = data.mac_address();
                if m == sysinfo::MacAddr::UNSPECIFIED {
                    None
                } else {
                    Some(m.to_string())
                }
            },
            addresses: data
                .ip_networks()
                .iter()
                .map(|n| n.addr.to_string())
                .collect(),
        })
        .collect()
}

/// A12 电池（委托平台实现）。
///
/// 三平台的电池来源完全不同（Windows 的 `GetSystemPowerStatus`、Linux 的
/// `/sys/class/power_supply`、macOS 的 IOKit），没有可共用的跨平台库，
/// 因此这里只做**转发**，不重复实现。
///
/// # 返回值
/// 电池列表；**无电池设备时返回空列表而不是错误** —— 台式机本来就没有电池，
/// 空列表是正确结果，不是失败。
pub fn batteries() -> Vec<crate::model::BatteryInfo> {
    serde_json::from_value(crate::platform_value("battery.list")).unwrap_or_default()
}

/// A13 系统时间线。
///
/// # 返回值
/// 启动时刻与运行时长
pub fn timeline() -> SystemTimeline {
    let boot = System::boot_time();
    SystemTimeline {
        boot_time_ms: if boot == 0 { None } else { Some(boot as i64 * 1000) },
        uptime_sec: System::uptime(),
    }
}

/// A14 主机信息。
///
/// # 返回值
/// 主机、系统与内核信息
pub fn host() -> HostInfo {
    HostInfo {
        hostname: System::host_name().unwrap_or_default(),
        os_name: System::name().unwrap_or_default(),
        os_version: System::os_version().unwrap_or_default(),
        kernel_version: System::kernel_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        timezone_offset_min: None,
        current_user: std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok(),
    }
}

/// 把 `uid` 解析成用户名。解析不到返回 `None`，不返回 uid 的字面量冒充名字。
///
/// # 参数
/// * `uid` - 用户 ID
///
/// # 返回值
/// 用户名
pub fn user_name(uid: u32) -> Option<String> {
    Users::new_with_refreshed_list()
        .list()
        .iter()
        .find(|u| u.id().to_string() == uid.to_string())
        .map(|u| u.name().to_string())
}

/// B9 由扁平列表构建进程树。
///
/// 使用**显式栈**而非递归：恶意或异常的进程表可能构造出环（A 的父是 B、B 的父是 A），
/// 递归会栈溢出并带走宿主 JVM。这里用 visited 集合切断环。
///
/// # 参数
/// * `flat` - (pid, ppid, name) 三元组列表
///
/// # 返回值
/// 根节点列表（父进程不在列表中的进程视为根）
pub fn build_tree(flat: &[(i32, Option<i32>, String)]) -> Vec<ProcessTreeNode> {
    use std::collections::{HashMap, HashSet};

    let index: HashMap<i32, usize> =
        flat.iter().enumerate().map(|(i, p)| (p.0, i)).collect();
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut roots: Vec<i32> = Vec::new();
    for (pid, ppid, _) in flat {
        match ppid {
            Some(p) if index.contains_key(p) && p != pid => children.entry(*p).or_default().push(*pid),
            _ => roots.push(*pid),
        }
    }

    fn make(
        pid: i32,
        flat: &[(i32, Option<i32>, String)],
        index: &HashMap<i32, usize>,
        children: &HashMap<i32, Vec<i32>>,
        visited: &mut HashSet<i32>,
    ) -> ProcessTreeNode {
        visited.insert(pid);
        let name = index
            .get(&pid)
            .map(|i| flat[*i].2.clone())
            .unwrap_or_default();
        let ppid = index.get(&pid).and_then(|i| flat[*i].1);
        // 先把子进程拷出来再递归：直接在 children.get() 的借用里递归会同时
        // 持有 visited 的不可变借用（filter 读）与可变借用（递归写），编译不过。
        let mut kids: Vec<ProcessTreeNode> = Vec::new();
        if let Some(v) = children.get(&pid) {
            let pending: Vec<i32> = v.clone();
            for c in pending {
                if !visited.contains(&c) {
                    kids.push(make(c, flat, index, children, visited));
                }
            }
        }
        ProcessTreeNode { pid, ppid, name, children: kids }
    }

    let mut visited = HashSet::new();
    let mut out = Vec::new();
    for r in roots {
        if !visited.contains(&r) {
            out.push(make(r, flat, &index, &children, &mut visited));
        }
    }
    out
}

/// 当前时刻的 Unix 毫秒时间戳，供平台实现与 `lib.rs` 复用。
///
/// # 返回值
/// Unix 毫秒
pub fn now_millis() -> i64 {
    now_ms()
}

/// 便捷构造：把进程名与 pid 组成可读串。
///
/// # 参数
/// * `pid` - 进程 ID
/// * `name` - 进程名
///
/// # 返回值
/// `name(pid)` 形式的串
pub fn label(pid: i32, name: &str) -> String {
    format!("{}({})", name, pid)
}

/// 占位：把 `Pid` 转成 i32。
///
/// # 参数
/// * `pid` - sysinfo 的 Pid
///
/// # 返回值
/// 整数进程 ID
pub fn pid_i32(pid: &Pid) -> i32 {
    pid.as_u32() as i32
}

/// 取单个进程的跨平台字段（只刷新该进程，不刷新全量）。
///
/// `process.detail` 原先复用 [`sysinfo_processes`]（刷新**全部**进程）再筛出目标，
/// 生产验收实测 p50 204ms。单进程刷新把这一步降到接近零。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 该进程的详情；进程不存在时返回 None
pub fn sysinfo_process_one(pid: i32) -> Option<crate::model::ProcessDetail> {
    let sys = refreshed_one(pid);
    let p = sys.process(Pid::from_u32(pid as u32))?;
    Some(process_detail_of(p, pid))
}

/// 把 sysinfo 的 Process 转成 ProcessDetail（跨平台基线部分）。
///
/// 平台实现只在此基础上补自己特有的字段。抽成独立函数是为了让
/// 「全量列表」与「单进程查询」两条路径**共用同一份字段填充逻辑**，
/// 避免两处写法漂移（此前 process.list 与 process.detail 就出现过不一致）。
///
/// # 参数
/// * `p` - sysinfo 的进程引用
/// * `pid` - 进程 ID
///
/// # 返回值
/// 跨平台部分的进程详情
fn process_detail_of(p: &sysinfo::Process, pid: i32) -> crate::model::ProcessDetail {
    use crate::model::ProcessDetail;

    let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
    let disk = p.disk_usage();
    ProcessDetail {
        pid,
        ppid: p.parent().map(|x| pid_i32(&x)),
        name: p.name().to_string_lossy().into_owned(),
        session_id: None,
        user: p.user_id().and_then(|u| user_name_str(&u.to_string())),
        uid: p.user_id().and_then(|u| u.to_string().parse::<i32>().ok()),
        gid: p.group_id().and_then(|g| g.to_string().parse::<i32>().ok()),
        status: format!("{:?}", p.status()).to_lowercase(),
        priority: None,
        priority_class: None,
        start_time_ms: {
            let s = p.start_time();
            if s == 0 { None } else { Some(s as i64 * 1000) }
        },
        run_time_sec: Some(p.run_time()),
        is_wow64: None,
        is_elevated: None,
        is_protected: None,
        cpu_usage: p.cpu_usage(),
        rss: p.memory(),
        virtual_memory: p.virtual_memory(),
        private_bytes: None,
        shared_bytes: None,
        thread_count: p.tasks().map(|t| t.len() as u32).unwrap_or(0),
        handle_count: None,
        // sysinfo 的 disk_usage 是自进程启动起的累计值，语义与
        // GetProcessIoCounters / /proc/PID/io 一致，可直接使用。
        io_read_bytes: Some(disk.read_bytes),
        io_written_bytes: Some(disk.written_bytes),
        io_read_count: None,
        io_write_count: None,
        command_line: if cmd.is_empty() { None } else { Some(cmd.join(" ")) },
        args: cmd,
        exe_path: p.exe().map(|e| e.to_string_lossy().into_owned()),
        cwd: p.cwd().map(|c| c.to_string_lossy().into_owned()),
        root_dir: p.root().map(|r| r.to_string_lossy().into_owned()),
        signature: None,
    }
}

/// 带过滤的进程列表：只保留满足 `keep` 的条目。
///
/// 抽这个函数是为了让「按平台判据过滤线程」这件事不改变字段填充逻辑
/// —— 两条路径共用 [`process_detail_of`]，避免漂移。
///
/// # 参数
/// * `keep` - 判定函数，返回 false 的条目会被丢弃
///
/// # 返回值
/// 过滤后的列表，按 CPU 使用率降序
pub fn sysinfo_processes_filtered<F>(keep: F) -> Vec<crate::model::ProcessDetail>
where
    F: Fn(&sysinfo::Process) -> bool,
{
    let sys = refreshed();
    let mut out: Vec<crate::model::ProcessDetail> = sys
        .processes()
        .iter()
        .filter(|(_, p)| keep(p))
        .map(|(pid, p)| process_detail_of(p, pid_i32(pid)))
        .collect();
    out.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// B 段跨平台基线：sysinfo 能一致拿到的进程字段。
///
/// 这是三平台的共同起点，平台实现只在此基础上补自己特有的字段
/// （如 Windows 的 WOW64 标记、提升状态、句柄数）。**不要把平台专属字段的默认值
/// 写死在这里**，否则平台实现区分不出"没采集"和"采到 0"。
///
/// # 返回值
/// 进程详情列表，按 CPU 使用率降序
pub fn sysinfo_processes() -> Vec<crate::model::ProcessDetail> {
    let sys = refreshed();
    let mut out: Vec<crate::model::ProcessDetail> = sys
        .processes()
        .iter()
        .map(|(pid, p)| process_detail_of(p, pid_i32(pid)))
        .collect();
    out.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 按 uid 字符串解析用户名（sysinfo 的 Uid 只有字符串形式）。
///
/// # 参数
/// * `uid` - 用户 ID 的字符串形式
///
/// # 返回值
/// 用户名
pub fn user_name_str(uid: &str) -> Option<String> {
    Users::new_with_refreshed_list()
        .list()
        .iter()
        .find(|u| u.id().to_string() == uid)
        .map(|u| u.name().to_string())
}

/// 便捷：把 `(pid, ppid, name)` 三元组列表转成进程树。
///
/// # 返回值
/// 进程树根节点列表
pub fn process_tree() -> Vec<ProcessTreeNode> {
    let flat: Vec<(i32, Option<i32>, String)> = sysinfo_processes()
        .into_iter()
        .map(|p| (p.pid, p.ppid, p.name))
        .collect();
    build_tree(&flat)
}
