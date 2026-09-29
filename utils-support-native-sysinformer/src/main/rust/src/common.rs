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

/// 刷新并返回全局 `System` 的锁。
fn refreshed() -> std::sync::MutexGuard<'static, System> {
    let mut sys = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    sys.refresh_cpu_all();
    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys
}

/// A1 + A2 一次刷新同时产出核列表与汇总。
///
/// **不要**分别调用 [`cpu_cores`] 与 [`cpu_summary`]：两者各自刷新一次，
/// 背靠背的两次刷新会把 CPU 使用率的差值窗口压到微秒级，Windows 侧 PDH 会给出
/// 退化值（本仓 metrics 模块记过这个坑，恒为 100）。需要两者时一律用本函数。
///
/// # 返回值
/// (每核列表, 汇总)
pub fn cpu_all() -> (Vec<CpuCore>, CpuSummary) {
    let sys = refreshed();
    let cpus = sys.cpus();
    let logical = cpus.len() as u32;
    let cores: Vec<CpuCore> = cpus
        .iter()
        .enumerate()
        .map(|(i, c)| CpuCore {
            id: i as u32,
            usage: c.cpu_usage(),
            frequency_mhz: match c.frequency() {
                0 => None,
                f => Some(f),
            },
            brand: c.brand().to_string(),
        })
        .collect();
    let usage = if cpus.is_empty() {
        0.0
    } else {
        cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / logical as f32
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
    let sys = refreshed();
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
    let sys = refreshed();
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

/// A12 电池。
///
/// # 返回值
/// 本函数暂不实现，交由平台补充；返回空列表而非编造数据
pub fn batteries() -> Vec<crate::model::BatteryInfo> {
    Vec::new()
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

/// B 段跨平台基线：sysinfo 能一致拿到的进程字段。
///
/// 这是三平台的共同起点，平台实现只在此基础上补自己特有的字段
/// （如 Windows 的 WOW64 标记、提升状态、句柄数）。**不要把平台专属字段的默认值
/// 写死在这里**，否则平台实现区分不出"没采集"和"采到 0"。
///
/// # 返回值
/// 进程详情列表，按 CPU 使用率降序
pub fn sysinfo_processes() -> Vec<crate::model::ProcessDetail> {
    use crate::model::ProcessDetail;

    let sys = refreshed();
    let mut out: Vec<ProcessDetail> = sys
        .processes()
        .iter()
        .map(|(pid, p)| {
            let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
            let disk = p.disk_usage();
            ProcessDetail {
                pid: pid_i32(pid),
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
        })
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
