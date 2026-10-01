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
//! # 其它平台
//!
//! Linux / macOS 的 `sysinfo` 口径与各自系统工具一致，故本文件**只用于
//! Windows**；其它平台仍走 [`crate::common`] 的 `sysinfo` 路径。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use windows::core::PCWSTR;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
};
use windows::Win32::System::Threading::{GetActiveProcessorCount, ALL_PROCESSOR_GROUPS};

/// PDH 查询与计数器句柄（windows crate 用 `isize` 承载 `PDH_HQUERY`）。
static QUERY: Lazy<Mutex<Option<(isize, Vec<isize>)>>> = Lazy::new(|| Mutex::new(None));

/// 进程内是否已建立 PDH 基线（计数器需要两个采集周期才有值）。
static READY: AtomicBool = AtomicBool::new(false);

/// PDH 成功码。
const PDH_SUCCESS: u32 = 0;

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
/// # 返回值
/// `(汇总使用率, 每核使用率)`，均为 0..100；取不到时返回 `None`
fn sample() -> Option<(f32, Vec<f32>)> {
    let mut guard = QUERY.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = open_query();
    }
    let (q, counters) = guard.as_ref()?;
    unsafe {
        PdhCollectQueryData(*q);
        let read = |c: &isize| -> Option<f32> {
            let mut val = PDH_FMT_COUNTERVALUE::default();
            let st = PdhGetFormattedCounterValue(*c, PDH_FMT_DOUBLE, None, &mut val);
            if st != PDH_SUCCESS {
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
        let cores: Vec<f32> = counters[1..].iter().map(|c| read(c).unwrap_or(0.0)).collect();
        Some((total, cores))
    }
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
    let mut guard = QUERY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((q, _)) = guard.take() {
        unsafe {
            PdhCloseQuery(q);
        }
    }
    READY.store(false, Ordering::Relaxed);
}
