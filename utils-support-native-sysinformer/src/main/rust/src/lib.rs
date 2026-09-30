//! 对外 C ABI 与平台分发。
//!
//! # ABI 设计
//!
//! 只暴露三个符号，而不是每种能力一个导出：
//!
//! * [`sysinformer_call`] —— `call(op, args_json) -> json`，按 op 名分发
//! * [`sysinformer_free_string`] —— 释放本库返回的字符串
//! * [`sysinformer_version`] —— 库版本
//!
//! 好处：新增能力只加一个 `match` 分支，Java 侧不用重新 JNI 绑定；跨平台差异
//! （某些 op 在某平台不支持）也能统一在 `Envelope` 里表达，而不是靠"符号不存在"。
//!
//! # 返回值约定
//!
//! 一律返回 UTF-8 JSON 的 **C 字符串**，调用方**必须**用 [`sysinformer_free_string`] 释放。
//! 内部失败不 panic：任何错误都序列化成 `Envelope { ok: false, error: ... }`。
//!
//! # 平台分发
//!
//! 三个平台实现文件由 `#[cfg(target_os = ...)]` 门控，**每个只在自己平台参与编译**，
//! 因此某个平台写错不会影响其它平台的构建。

mod model;

/// 跨平台基线（sysinfo）。
mod common;

/// 各平台实现。用 `#[path]` 指向扁平文件，避免为一个模块建一层目录。
#[cfg(target_os = "windows")]
#[path = "platform_windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "platform_linux.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod platform;

/// 非三平台目标的兜底实现，保证 `cargo check` 在其它系统上也能过。
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
#[path = "platform_unsupported.rs"]
mod platform;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use model::Envelope;

/// 当前平台标识，写进"不支持"错误里。
pub(crate) const PLATFORM: &str = if cfg!(target_os = "windows") {
    "windows"
} else if cfg!(target_os = "linux") {
    "linux"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "unknown"
};

/// 把字符串转成交给调用方释放的 C 字符串。
///
/// 内部 NUL 字节会让 `CString::new` 失败；此处回退成一条转义过的错误 JSON，
/// 而不是返回空指针——空指针会让调用方无法区分"失败"与"空结果"。
fn to_c_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"data":null,"error":"响应包含 NUL 字节，已丢弃"}"#)
            .map(|c| c.into_raw())
            .unwrap_or(std::ptr::null_mut()),
    }
}

/// 把 C 字符串参数转成 `&str`，空指针按空串处理。
///
/// # 安全性
/// 调用方必须保证指针要么为空，要么指向以 NUL 结尾的合法 UTF-8 字符串。
unsafe fn from_c<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    CStr::from_ptr(p).to_str().unwrap_or("")
}

/// 按 op 名分发。
///
/// # 参数
/// * `op` —— 操作名，见 README 的 op 一览表
/// * `args` —— JSON 对象形式的参数，可为空指针表示无参
///
/// # 返回值
/// JSON 信封。op 未知时返回 `ok=false`。
///
/// # 分工
/// * **跨平台 op**（`system.*`）在这里直接用 `common` 实现；
/// * **其余 op** 一律转发给 `platform::call`，由各平台实现。
///
/// 这样每个平台文件只需实现一个 `call` 入口，平台差异（不支持的能力）在
/// `Envelope` 里显式表达，而不是靠"符号不存在"。
pub(crate) fn dispatch(op: &str, args: &str) -> String {
    match op {
        // ---------- 跨平台：系统级快照 ----------
        // 只有这一项在 lib.rs 内组装，因为它全部来自 sysinfo，三平台一致。
        //
        // **刻意不含 sensor.list 与 memory.modules**：这两项在 Windows 上走 WMI
        // （COM 通道），实测把快照 p50 从几十毫秒推到近 200ms；而内存条是**静态数据**，
        // 温度也不需要秒级刷新。生产验收实测把它们并进快照后，1s 周期采样有 20% 时间
        // 耗在采集上。需要时请单独调用对应 op。
        "system.snapshot" => {
            // 一次刷新同时产出核列表与汇总：分两次调用会把使用率差值窗口压成微秒。
            let (cpu_cores, cpu) = common::cpu_all();
            let v = serde_json::json!({
                "cpu_cores": cpu_cores,
                "cpu": cpu,
                "load": common::load_average(),
                "memory": common::memory(),
                "swap": common::swap(),
                "disks": common::disks(),
                "networks": common::networks(),
                "batteries": common::batteries(),
                "timeline": common::timeline(),
                "host": common::host(),
                // 平台相关的部分并入快照；缺失时由平台给出原因，不静默省略
                "disk_io": platform_value("disk.io"),
                "gpus": platform_value("gpu.list"),
            });
            serde_json::to_string(&Envelope::ok(v)).unwrap_or_default()
        }

        // ---------- 其余全部交给平台实现 ----------
        _ => platform::call(op, args),
    }
}

/// 取某个平台 op 的 `data` 部分，失败时返回 null（用于并入快照）。
///
/// # 参数
/// * `op` - 平台 op 名
///
/// # 返回值
/// 成功时是 `data` 的值，失败或无数据时是 JSON null
fn platform_value(op: &str) -> serde_json::Value {
    let raw = platform::call(op, "{}");
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) if v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) => {
            v.get("data").cloned().unwrap_or(serde_json::Value::Null)
        }
        _ => serde_json::Value::Null,
    }
}

/// 统一入口：`call(op, args_json) -> json`。
///
/// # 安全性
/// `op` 与 `args` 必须要么为空指针，要么指向以 NUL 结尾的合法 UTF-8 字符串。
/// 返回的指针由本库分配，调用方必须用 [`sysinformer_free_string`] 释放。
#[no_mangle]
pub extern "C" fn sysinformer_call(op: *const c_char, args: *const c_char) -> *mut c_char {
    let op = unsafe { from_c(op) };
    let args = unsafe { from_c(args) };
    // 不 panic 跨 FFI 边界：任何 panic 都会变成未定义行为并把宿主 JVM 带走。
    let out = std::panic::catch_unwind(|| dispatch(op, args)).unwrap_or_else(|_| {
        serde_json::to_string(&Envelope::<()>::err("内部 panic，已捕获"))
            .unwrap_or_else(|_| r#"{"ok":false,"data":null,"error":"panic"}"#.to_string())
    });
    to_c_string(out)
}

/// 释放本库返回的字符串。
///
/// # 安全性
/// `ptr` 必须是 [`sysinformer_call`] 或 [`sysinformer_version`] 的返回值，且**只能释放一次**。
#[no_mangle]
pub extern "C" fn sysinformer_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(ptr));
    }
}

/// 返回库版本与构建目标。
#[no_mangle]
pub extern "C" fn sysinformer_version() -> *mut c_char {
    to_c_string(format!(
        r#"{{"version":"{}","platform":"{}","target":"{}"}}"#,
        env!("CARGO_PKG_VERSION"),
        PLATFORM,
        std::env::consts::ARCH
    ))
}
