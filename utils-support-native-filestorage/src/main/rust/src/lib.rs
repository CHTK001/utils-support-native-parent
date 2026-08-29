//! Rust 文件存储处理器 native 库
//! 提供文件处理能力的 native 实现

use std::ffi::{c_char, CStr, CString};
use std::os::raw::c_void;

// ==================== 内部状态 ====================

/// 文件存储处理器能力列表
const CAPABILITIES: &str = "resize,watermark,crop,rotate,filter,exclude";

/// 图像过滤器能力列表
const FILTER_CAPABILITIES: &str = "blur,sharpen,grayscale,flip,auto_orient";

/// 初始化状态
static mut INITIALIZED: bool = false;

// ==================== JNI 接口 ====================

/// 初始化
#[no_mangle]
pub extern "C" fn native_init() -> bool {
    unsafe {
        INITIALIZED = true;
        true
    }
}

/// 检查是否已初始化
#[no_mangle]
pub extern "C" fn native_is_initialized() -> bool {
    unsafe { INITIALIZED }
}

/// 获取版本
#[no_mangle]
pub extern "C" fn native_get_version() -> *mut c_char {
    unsafe {
        CString::new("1.0.0").unwrap().into_raw()
    }
}

/// 获取能力列表
#[no_mangle]
pub extern "C" fn native_get_capabilities() -> *mut c_char {
    unsafe {
        CString::new(CAPABILITIES).unwrap().into_raw()
    }
}

/// 解析参数 JSON
#[no_mangle]
pub extern "C" fn native_parse_params(params_json: *const c_char) -> *mut c_char {
    unsafe {
        if params_json.is_null() {
            return CString::new("{}").unwrap().into_raw();
        }
        let params = CStr::from_ptr(params_json).to_string_lossy().to_string();
        // 简单解析并返回原始 JSON
        CString::new(params).unwrap().into_raw()
    }
}

/// 获取过滤器能力
#[no_mangle]
pub extern "C" fn native_get_filter_capabilities() -> *mut c_char {
    unsafe {
        CString::new(FILTER_CAPABILITIES).unwrap().into_raw()
    }
}

/// 获取过滤器链 JSON
#[no_mangle]
pub extern "C" fn native_get_filter_chain_json() -> *mut c_char {
    unsafe {
        CString::new("[]").unwrap().into_raw()
    }
}

/// 检查是否被排除
#[no_mangle]
pub extern "C" fn native_is_excluded(path: *const c_char, extension: *const c_char) -> bool {
    unsafe {
        if path.is_null() || extension.is_null() {
            return false;
        }
        let ext = CStr::from_ptr(extension).to_string_lossy().to_lowercase();
        // 排除系统文件
        matches!(ext.as_str(), "tmp" | "temp" | "log" | "cache")
    }
}

// ==================== Free 函数 ====================

#[no_mangle]
pub extern "C" fn native_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}
