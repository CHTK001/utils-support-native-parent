//! Rust 无头浏览器动态库 —— 面向 Java FFM 的扁平 C ABI。
//!
//! 导出三个稳定符号（与 `utils-support-native-sysinformer` / `ffmpeg` 同一套约定）：
//!
//! * [`pw_call`] —— 传入请求 JSON 字符串，返回响应 JSON 字符串（堆分配）
//! * [`pw_free_string`] —— 释放本库返回的字符串（必须）
//! * [`pw_version`] —— 返回常量版本串（**无需**释放）
//!
//! 请求信封：`{"action":"goto","handle":1,"params":{...}}`，
//! 响应信封：`{"ok":...}` 或 `{"error":"..."}`。
//!
//! 同时保留旧的 `download_page` / `execute_script` / `screenshot` /
//! `screenshot_with_check` / `free_string` 五个符号 —— native-matrix CI 与
//! 旧调用方仍按这组名字做导出断言与调用，且它们现在是**真实实现**（不再是 stub）。

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_longlong};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

mod dispatch;
mod state;

/// 版本串（NUL 结尾，静态存储，释放无意义）。
static VERSION_CSTR: &str = concat!("headless-rust/", env!("CARGO_PKG_VERSION"), "\0");

/// 返回库版本。指针指向静态数据，调用方**不得**释放。
#[no_mangle]
pub extern "C" fn pw_version() -> *const c_char {
    VERSION_CSTR.as_ptr() as *const c_char
}

/// 把 Rust 字符串转成堆上 C 字符串（失败时返回一个错误 JSON，绝不返回 null）。
fn to_c_string(s: &str) -> *mut c_char {
    // JSON 里不会出现内嵌 NUL；万一出现，剥掉后续内容保证 CString 可构造
    let cleaned: String = s.chars().filter(|c| *c != '\u{0}').collect();
    match CString::new(cleaned) {
        Ok(c) => c.into_raw(),
        Err(_) => CString::new("{\"error\":\"response contains NUL\"}")
            .expect("static json")
            .into_raw(),
    }
}

/// 调度一个动作并返回响应 JSON 字符串（panic 被捕获，绝不让 panic 穿过 FFI）。
fn call_json(request: &str) -> *mut c_char {
    let resp = catch_unwind(AssertUnwindSafe(|| dispatch::dispatch(request)))
        .unwrap_or_else(|_| "{\"error\":\"panic inside pw_call\"}".to_string());
    to_c_string(&resp)
}

/// 执行一个动作，返回 `{"ok":...}` 里的结果值；出错返回 Err(错误文本)。
fn call_ok(request: &str) -> Result<serde_json::Value, String> {
    let resp_str = catch_unwind(AssertUnwindSafe(|| dispatch::dispatch(request)))
        .unwrap_or_else(|_| "{\"error\":\"panic inside dispatch\"}".to_string());
    let resp: serde_json::Value =
        serde_json::from_str(&resp_str).unwrap_or(serde_json::json!({"error": "响应非 JSON"}));
    match resp.get("error") {
        Some(e) if !e.is_null() => Err(e.as_str().unwrap_or("unknown error").to_string()),
        _ => Ok(resp.get("ok").cloned().unwrap_or(serde_json::Value::Null)),
    }
}

/// 读取 C 字符串指针（null 安全）。
unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

/// 主入口：`pw_call(request_json) -> response_json`。
///
/// 返回的指针必须用 [`pw_free_string`] 释放。null 入参返回错误 JSON（不返回 null）。
#[no_mangle]
pub unsafe extern "C" fn pw_call(request: *const c_char) -> *mut c_char {
    if request.is_null() {
        return to_c_string(r#"{"error":"pw_call: request is null"}"#);
    }
    let req = CStr::from_ptr(request).to_string_lossy().into_owned();
    call_json(&req)
}

/// 释放 [`pw_call`] 返回的字符串。null 安全。
#[no_mangle]
pub unsafe extern "C" fn pw_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    drop(CString::from_raw(s));
}

// ==================== 旧符号（兼容 + CI 导出断言） ====================

/// 旧入口：下载页面 HTML。失败返回 null（与旧契约一致），成功返回堆上 HTML。
///
/// 内部走 dispatch：launch → goto → content → close。
#[no_mangle]
pub unsafe extern "C" fn download_page(
    url: *const c_char,
    headers: *const c_char,
    cookies: *const c_char,
    user_agent: *const c_char,
    timeout: c_longlong,
) -> *mut c_char {
    let url = cstr(url);
    if url.is_empty() {
        return ptr::null_mut();
    }
    let headers = cstr(headers);
    let cookies = cstr(cookies);
    let ua = cstr(user_agent);
    let timeout = if timeout > 0 { timeout as u64 } else { 30_000 };
    let req = serde_json::json!({
        "action": "legacyDownloadPage",
        "params": {
            "url": url,
            "headers": headers,
            "cookies": cookies,
            "userAgent": ua,
            "timeout": timeout,
        }
    })
    .to_string();
    match call_ok(&req) {
        Ok(v) => to_c_string(v.get("value").and_then(serde_json::Value::as_str).unwrap_or("")),
        Err(e) => {
            log::error!("download_page failed: {e}");
            ptr::null_mut()
        }
    }
}

/// 旧入口：执行脚本并返回结果 JSON（原来恒返回 null，现在返回真实结果）。
#[no_mangle]
pub unsafe extern "C" fn execute_script(url: *const c_char, script: *const c_char) -> *mut c_char {
    let url = cstr(url);
    let script = cstr(script);
    if url.is_empty() || script.is_empty() {
        return ptr::null_mut();
    }
    let req = serde_json::json!({
        "action": "legacyExecuteScript",
        "params": { "url": url, "script": script }
    })
    .to_string();
    match call_ok(&req) {
        Ok(v) => to_c_string(&v.to_string()),
        Err(e) => {
            log::error!("execute_script failed: {e}");
            ptr::null_mut()
        }
    }
}

/// 旧入口：打开 url 并截图到 path。
#[no_mangle]
pub unsafe extern "C" fn screenshot(url: *const c_char, path: *const c_char) -> bool {
    screenshot_with_check(url, path, ptr::null(), 5000)
}

/// 旧入口：带内容校验与等待的截图。成功返回 true。
#[no_mangle]
pub unsafe extern "C" fn screenshot_with_check(
    url: *const c_char,
    path: *const c_char,
    check: *const c_char,
    wait: c_longlong,
) -> bool {
    let url = cstr(url);
    let path = cstr(path);
    if url.is_empty() || path.is_empty() {
        return false;
    }
    let req = serde_json::json!({
        "action": "legacyScreenshotUrl",
        "params": {
            "url": url,
            "path": path,
            "check": cstr(check),
            "waitMs": if wait > 0 { wait as u64 } else { 5000u64 },
        }
    })
    .to_string();
    match call_ok(&req) {
        Ok(_) => true,
        Err(e) => {
            log::error!("screenshot failed: {e}");
            false
        }
    }
}

/// 旧入口：释放 `download_page` / `execute_script` 返回的字符串。null 安全。
#[no_mangle]
pub unsafe extern "C" fn free_string(s: *mut c_char) {
    pw_free_string(s);
}
