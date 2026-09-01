//! 页面操作模块（stub）

use std::ptr;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_longlong};
use anyhow::Result;

pub fn download_page(_url: &str, _headers: &serde_json::Value, _cookies: &serde_json::Value, _ua: &str, _timeout: u64) -> Result<String> {
    Ok("<html><body>stub</body></html>".to_string())
}

pub fn screenshot(_url: &str, _path: &str, _check: Option<&str>, _wait: u64) -> Result<bool> {
    Ok(false)
}
