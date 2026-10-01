//! Windows 上按**任务管理器同源口径**读取 CPU 使用率。
//!
//! # 为什么不能直接用 `sysinfo` 的 `cpu_usage()`
//!
//! `sysinfo` 0.33.1 在 Windows 上只注册 **`% Idle Time`** 计数器，
//! 用 `100.0 - idle` 算出使用率（其 `src/windows/system.rs`：
//! `add_english_counter(r"\Processor(_Total)\% Idle Time", ...)` 与
//! `self.cpus.global.set_cpu_usage(100.0 - total_idle_time)`）。
//!
//! 而任务管理器用 **`% Processor Time`**，两者分母不同：
//! - `% Idle Time` 的分母是**全部时间**（含 idle）
//! - `% Processor Time` 的分母是**非 idle 时间**
//!
//! 因此 `100 - idle` 与 `% Processor Time` 在有内核态活动（中断、DPC、
//! 系统调用）时**必然不同**。实测本机 20 组采样：本库旧口径相对任务管理器
//! **系统性偏高 +5.17pp**，95% 置信区间 `[+1.01, +9.33]pp`（不含 0），
//! 是真实偏差而非采样噪声。
//!
//! 本模块按任务管理器口径直接读 `% Processor Time`，使调用方拿到的数字
//! 能与任务管理器、性能监控、告警阈值直接对齐。
//!
//! # 短间隔采集会读到陈旧值（2026-10-02 实测发现的真实缺陷）
//!
//! `% Processor Time` 的**原始采样时间戳按系统定时器节拍推进**（经典值
//! 15.625ms）。若两次 `PdhCollectQueryData` 的间隔短于节拍，两次会拿到
//! 同一个时间戳，PDH 算出**非正分母**，返回
//! `PDH_CALC_NEGATIVE_DENOMINATOR`（`0x800007D6`），并且**不更新**
//! `doubleValue` —— 它仍是上一次的值。
//!
//! 实测（Windows 10 / 12 核 / `\Processor(_Total)\% Processor Time`，
//! 每档 40s）：
//!
//! ```text
//! 采集间隔ms   非 VALID 读数占比
//!         30                 0.23%
//!         50                 7.94%   <- 比 30ms 差得多，故 50ms 不可用
//!        100                 0.00%
//!        200 / 400           0.00%
//! ```
//!
//! 失败率**不是**间隔的单调函数：它取决于**当时的系统定时器节拍**，
//! 而 Windows 会做节拍合并（可到 31.25 / 62.5ms）。所以不能从
//! 「30ms 干净」推断「50ms 也干净」—— 50ms 已被实测证伪。
//!
//! 本模块原先只检查 `PdhGetFormattedCounterValue` 的 API 返回码、
//! **不检查 `val.CStatus`**，于是这类无效读数被原样当成真值使用。
//! 后果是 CPU 使用率出现「**稳定得反常**」的读数：真实值明明在 2%
//! 附近波动，读数却冻结在早先某个高负载时刻的 ~20% 上（陈旧值的方差
//! 天然接近 0）。CI 的 4 核 runner 上验收脚本按 ~23ms 采样，正落在
//! 失败区间内，观察到的正是这个形态。
//!
//! 现两处都堵上：**检查 `CStatus`**（正确性，根本性），且
//! **强制 100ms 最小采集间隔**（预防，见该常量文档）。
//!
//! # 其它平台
//!
//! Linux / macOS 的 `sysinfo` 口径与各自系统工具一致，故本文件**只用于
//! Windows**；其它平台仍走 [`crate::common`] 的 `sysinfo` 路径。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;
use windows::core::PCWSTR;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_CSTATUS_VALID_DATA,
    PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
};
use windows::Win32::System::Threading::{GetActiveProcessorCount, ALL_PROCESSOR_GROUPS};

/// PDH 查询与计数器句柄（windows crate 用 `isize` 承载 `PDH_HQUERY`）。
static QUERY: Lazy<Mutex<Option<(isize, Vec<isize>)>>> = Lazy::new(|| Mutex::new(None));

/// 上一次**全部计数器都有效**的读数（采集时刻、汇总、每核）。
///
/// 采集间隔不足、或 PDH 报非 VALID 的 `CStatus` 时复用它，
/// 宁可给一个跨度更长但真实的平均值，也不给一个未定义的值。
static LAST_OK: Lazy<Mutex<Option<(Instant, f32, Vec<f32>)>>> =
    Lazy::new(|| Mutex::new(None));

/// 进程内是否已建立 PDH 基线（计数器需要两个采集周期才有值）。
static READY: AtomicBool = AtomicBool::new(false);

/// PDH 成功码。
const PDH_SUCCESS: u32 = 0;

/// 两次 `PdhCollectQueryData` 之间的最小间隔（毫秒）。
///
/// 取 100 的依据是实测，不是拍脑袋。每档 40s、本机 12 核：
///
/// ```text
/// 采集间隔ms   非 VALID 读数占比
///         30                0.23%
///         50                7.94%    <- 注意：比 30ms 差得多
///        100                0.00%
///        200                0.00%
///        400                0.00%
/// ```
///
/// 失败率**不是**间隔的干净函数（50ms 反而比 30ms 差），因为它取决于
/// **当时的系统定时器节拍**：Windows 会做节拍合并，合并后可到
/// 31.25 / 62.5ms。短于当时节拍的间隔就会拿到重复的时间戳。
/// 因此**不能**从「30ms 干净」推断「50ms 也干净」—— 50ms 已被实测证伪。
/// 100ms 是第一个有干净记录的周期，且高于可能的合并节拍。
///
/// 更根本的一点：本条只是**预防**。即使间隔取得够长，`CStatus` 检查仍在，
/// 所以阈值取错不会导致把陈旧值当读数，只会让 CPU 读数稍微滞后一点。
/// 两者是「双保险」，不是二选一 —— 别为了「让数字更新鲜」把间隔调小。
///
/// 代价：调用方若快于 10Hz 采样 CPU，读到的是跨度 ≥100ms 的平均值。
/// 任务管理器默认刷新周期是 1s，本模块的建议采样周期也是 1s，
/// 故实际使用中几乎不会触发。
const MIN_COLLECT_INTERVAL_MS: u64 = 100;

/// 打开查询并注册 `% Processor Time` 的 `_Total` 与每核计数器。
///
/// # 返回值
/// `(查询句柄, 计数器列表)`；任一计数器注册失败则返回 `None` 并关闭查询
fn open_query() -> Option<(isize, Vec<isize>)> {
    unsafe {
        let mut q: isize = 0;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) != PDH_SUCCESS {
            return None;
        }
        let ncpu = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) as usize;
        if ncpu == 0 {
            PdhCloseQuery(q);
            return None;
        }
        let mut counters: Vec<isize> = Vec::with_capacity(ncpu + 1);
        let mut ok = true;
        let mut add = |path: &str| {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut c: isize = 0;
            let rc = PdhAddEnglishCounterW(q, PCWSTR(wide.as_ptr()), 0, &mut c);
            if rc == PDH_SUCCESS {
                counters.push(c);
            } else {
                ok = false;
            }
        };
        add(r"\Processor(_Total)\% Processor Time");
        for i in 0..ncpu {
            add(&format!(r"\Processor({i})\% Processor Time"));
        }
        if !ok || counters.is_empty() {
            PdhCloseQuery(q);
            return None;
        }
        Some((q, counters))
    }
}

/// 采集一次 `[汇总, 每核...]` 的使用率。
///
/// # 短路与复用
///
/// 1. 距上次采集不足 [`MIN_COLLECT_INTERVAL_MS`] 时**不采集**，直接复用
///    上一次有效读数。短于定时器节拍时 PDH 会返回非正分母且不更新值，
///    采集本身就没有意义。
/// 2. 采集返回码非成功、或任一计数器的 `CStatus` 不是
///    `PDH_CSTATUS_VALID_DATA` 时，同样复用上一次有效读数。
///    **绝不使用 `CStatus` 无效时的 `doubleValue`** —— 那个值是上一次的
///    陈旧值，用它会让 CPU 使用率冻结在一个早已不成立的数字上。
///
/// # 返回值
/// `(汇总使用率, 每核使用率)`，均为 0..100；从未有过有效读数时返回 `None`
fn sample() -> Option<(f32, Vec<f32>)> {
    {
        let last = LAST_OK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, total, cores)) = last.as_ref() {
            if at.elapsed().as_millis() < MIN_COLLECT_INTERVAL_MS as u128 {
                return Some((*total, cores.clone()));
            }
        }
    }

    let mut guard = QUERY.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = open_query();
    }
    let (q, counters) = guard.as_ref()?;

    let fresh = unsafe { collect_and_read(*q, counters) };
    match fresh {
        Some((total, cores)) => {
            let mut last = LAST_OK.lock().unwrap_or_else(|e| e.into_inner());
            *last = Some((Instant::now(), total, cores.clone()));
            Some((total, cores))
        }
        // 采集或读失败：退回上一次有效值；连一次都没有才返回 None
        None => LAST_OK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(_, t, c)| (*t, c.clone())),
    }
}

/// 执行一次采集并读取全部计数器。
///
/// # 返回值
/// 全部计数器都有效时返回 `(汇总, 每核)`；**任一个** `CStatus` 非
/// `PDH_CSTATUS_VALID_DATA` 即返回 `None`（宁可不取，也不拼凑一个
/// 含无效分量的列表）。
unsafe fn collect_and_read(q: isize, counters: &[isize]) -> Option<(f32, Vec<f32>)> {
    if PdhCollectQueryData(q) != PDH_SUCCESS {
        return None;
    }
    let read = |c: &isize| -> Option<f32> {
        let mut val = PDH_FMT_COUNTERVALUE::default();
        let st = PdhGetFormattedCounterValue(*c, PDH_FMT_DOUBLE, None, &mut val);
        // CStatus 必须单独判断：API 返回 PDH_SUCCESS 时 CStatus 仍可能是
        // PDH_CALC_NEGATIVE_DENOMINATOR(0x800007D6) 等，此时 PDH **没有**
        // 更新 doubleValue，它还是上一次的值。
        if st != PDH_SUCCESS || val.CStatus != PDH_CSTATUS_VALID_DATA {
            return None;
        }
        let d = val.Anonymous.doubleValue;
        if d.is_finite() {
            Some(d.clamp(0.0, 100.0) as f32)
        } else {
            None
        }
    };
    let total = read(&counters[0])?;
    let mut cores = Vec::with_capacity(counters.len() - 1);
    for c in &counters[1..] {
        cores.push(read(c)?);
    }
    Some((total, cores))
}

/// 按任务管理器口径返回 `(汇总使用率, 每核使用率)`。
///
/// 首次调用会连采两次建立基线（PDH 需要两个 `PdhCollectQueryData`
/// 才有值），并做一次短等待让窗口非零。
///
/// # 返回值
/// `(汇总, 每核)`；PDH 不可用时返回 `(0.0, 空)`，调用方据此回退到 `sysinfo`
pub fn cpu_usage() -> (f32, Vec<f32>) {
    if !READY.load(Ordering::Relaxed) {
        // 第一次只建立基线，丢弃其结果。
        let _ = sample();
        std::thread::sleep(std::time::Duration::from_millis(120));
        let r = sample();
        READY.store(true, Ordering::Relaxed);
        return r.unwrap_or((0.0, Vec::new()));
    }
    sample().unwrap_or((0.0, Vec::new()))
}

/// 关闭 PDH 查询。仅在进程退出时调用。
pub fn shutdown() {
    {
        let mut last = LAST_OK.lock().unwrap_or_else(|e| e.into_inner());
        *last = None;
    }
    let mut guard = QUERY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((q, _)) = guard.take() {
        unsafe {
            PdhCloseQuery(q);
        }
    }
    READY.store(false, Ordering::Relaxed);
}
