//! Rust 无头浏览器库（stub：headless_chrome 未集成）

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_longlong};
use std::ptr;

mod browser;
mod page;

#[no_mangle]
pub unsafe extern "C" fn download_page(
    url: *const c_char,
    _headers: *const c_char,
    _cookies: *const c_char,
    _user_agent: *const c_char,
    _timeout: c_longlong,
) -> *mut c_char {
    let url_str = if url.is_null() { String::new() } else { CStr::from_ptr(url).to_string_lossy().into() };
    match page::download_page(&url_str, &serde_json::json!({}), &serde_json::json!({}), "", 0) {
        Ok(html) => match CString::new(html) { Ok(s) => s.into_raw(), Err(_) => ptr::null_mut() },
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn execute_script(_url: *const c_char, _script: *const c_char) -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn screenshot(url: *const c_char, path: *const c_char) -> bool {
    screenshot_with_check(url, path, ptr::null(), 5000)
}

#[no_mangle]
pub unsafe extern "C" fn screenshot_with_check(
    url: *const c_char, path: *const c_char, _check: *const c_char, _wait: c_longlong,
) -> bool {
    let url_str = if url.is_null() { String::new() } else { CStr::from_ptr(url).to_string_lossy().into() };
    let path_str = if path.is_null() { String::new() } else { CStr::from_ptr(path).to_string_lossy().into() };
    matches!(page::screenshot(&url_str, &path_str, None, 0), Ok(true))
}

#[no_mangle]
pub unsafe extern "C" fn free_string(ptr: *mut c_char) {
    if ptr.is_null() { return; }
    let _ = CString::from_raw(ptr);
}
