//! Rust 鏃犲ご娴忚鍣ㄥ簱
//! 
//! 鎻愪緵楂樻€ц兘鐨勬祻瑙堝櫒鑷姩鍖栬兘鍔涳紝閫氳繃 FFI 鎺ュ彛渚?Java 璋冪敤

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_longlong};
use std::ptr;

mod browser;
mod page;

use page::{download_page as rust_download_page, screenshot as rust_screenshot};

/// 涓嬭浇椤甸潰
/// 
/// # Safety
/// 
/// 姝ゅ嚱鏁伴€氳繃 FFI 璋冪敤锛岄渶瑕佺‘淇濅紶鍏ョ殑鎸囬拡鏈夋晥
/// 浣跨敤 catch_unwind 闃叉 panic 瀵艰嚧 JVM 宕╂簝
#[no_mangle]
pub unsafe extern "C" fn download_page(
    url: *const c_char,
    headers: *const c_char,
    cookies: *const c_char,
    user_agent: *const c_char,
    timeout: c_longlong,
) -> *mut c_char {
    // 浣跨敤 catch_unwind 鎹曡幏 panic锛岄槻姝㈠鑷?JVM 宕╂簝
    let result = std::panic::catch_unwind(|| {
        // 杞崲 C 瀛楃涓蹭负 Rust 瀛楃涓?        let url_str = match c_str_to_string(url) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[涓嬭浇]瑙ｆ瀽 URL 澶辫触: {}", e);
                return ptr::null_mut();
            }
        };
        
        let headers_str = match c_str_to_string(headers) {
            Ok(s) => s,
            Err(_) => "{}".to_string(),
        };
        
        let cookies_str = match c_str_to_string(cookies) {
            Ok(s) => s,
            Err(_) => "{}".to_string(),
        };
        
        let user_agent_str = match c_str_to_string(user_agent) {
            Ok(s) => s,
            Err(_) => "".to_string(),
        };
        
        // 瑙ｆ瀽 JSON
        let headers_map: serde_json::Value = serde_json::from_str(&headers_str)
            .unwrap_or_else(|_| serde_json::json!({}));
        let cookies_map: serde_json::Value = serde_json::from_str(&cookies_str)
            .unwrap_or_else(|_| serde_json::json!({}));
        
        // 璋冪敤 Rust 瀹炵幇
        match rust_download_page(
            &url_str,
            &headers_map,
            &cookies_map,
            &user_agent_str,
            timeout as u64,
        ) {
            Ok(html) => {
                // 杞崲涓?C 瀛楃涓?                match CString::new(html) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(e) => {
                        log::error!("[鏃犲ご娴忚鍣╙[涓嬭浇]杞崲缁撴灉瀛楃涓插け璐? {}", e);
                        ptr::null_mut()
                    }
                }
            }
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[涓嬭浇]涓嬭浇椤甸潰澶辫触: {}", e);
                ptr::null_mut()
            }
        }
    });
    
    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            log::error!("[鏃犲ご娴忚鍣╙[涓嬭浇]鍙戠敓 panic锛屽凡鎹曡幏");
            ptr::null_mut()
        }
    }
}

/// 鎵ц JavaScript
/// 
/// # Safety
/// 
/// 姝ゅ嚱鏁伴€氳繃 FFI 璋冪敤锛岄渶瑕佺‘淇濅紶鍏ョ殑鎸囬拡鏈夋晥
/// 浣跨敤 catch_unwind 闃叉 panic 瀵艰嚧 JVM 宕╂簝
#[no_mangle]
pub unsafe extern "C" fn execute_script(
    url: *const c_char,
    script: *const c_char,
) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        let url_str = match c_str_to_string(url) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[鑴氭湰]瑙ｆ瀽 URL 澶辫触: {}", e);
                return ptr::null_mut();
            }
        };
        
        let script_str = match c_str_to_string(script) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[鑴氭湰]瑙ｆ瀽鑴氭湰澶辫触: {}", e);
                return ptr::null_mut();
            }
        };
        
        // TODO: 瀹炵幇 JavaScript 鎵ц
        log::warn!("[鏃犲ご娴忚鍣╙[鑴氭湰]execute_script 灏氭湭瀹炵幇: url={}, script={}", url_str, script_str);
        ptr::null_mut()
    });
    
    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            log::error!("[鏃犲ご娴忚鍣╙[鑴氭湰]鍙戠敓 panic锛屽凡鎹曡幏");
            ptr::null_mut()
        }
    }
}

/// 鎴浘
/// 
/// # Safety
/// 
/// 姝ゅ嚱鏁伴€氳繃 FFI 璋冪敤锛岄渶瑕佺‘淇濅紶鍏ョ殑鎸囬拡鏈夋晥
/// 浣跨敤 catch_unwind 闃叉 panic 瀵艰嚧 JVM 宕╂簝
#[no_mangle]
pub unsafe extern "C" fn screenshot(
    url: *const c_char,
    path: *const c_char,
) -> bool {
    screenshot_with_check(url, path, ptr::null(), 5000)
}

/// 鎴浘锛堝甫椤甸潰鍔犺浇瀹屾垚妫€鏌ワ級
/// 
/// # Safety
/// 
/// 姝ゅ嚱鏁伴€氳繃 FFI 璋冪敤锛岄渶瑕佺‘淇濅紶鍏ョ殑鎸囬拡鏈夋晥
/// 浣跨敤 catch_unwind 闃叉 panic 瀵艰嚧 JVM 宕╂簝
#[no_mangle]
pub unsafe extern "C" fn screenshot_with_check(
    url: *const c_char,
    path: *const c_char,
    check_script: *const c_char,
    max_wait_time: c_longlong,
) -> bool {
    let result = std::panic::catch_unwind(|| {
        let url_str = match c_str_to_string(url) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[鎴浘]瑙ｆ瀽 URL 澶辫触: {}", e);
                return false;
            }
        };
        
        let path_str = match c_str_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[鎴浘]瑙ｆ瀽璺緞澶辫触: {}", e);
                return false;
            }
        };
        
        // 瑙ｆ瀽妫€鏌ヨ剼鏈紙鍙€夛級
        let check_script_opt = if check_script.is_null() {
            None
        } else {
            match c_str_to_string(check_script) {
                Ok(s) if !s.is_empty() => Some(s),
                _ => None,
            }
        };
        
        // 璋冪敤 Rust 瀹炵幇
        match rust_screenshot(
            &url_str,
            &path_str,
            check_script_opt.as_deref(),
            max_wait_time as u64,
        ) {
            Ok(success) => success,
            Err(e) => {
                log::error!("[鏃犲ご娴忚鍣╙[鎴浘]鎴浘澶辫触: {}", e);
                false
            }
        }
    });
    
    result.unwrap_or(false)
}

/// 閲婃斁瀛楃涓插唴瀛?/// 
/// # Safety
/// 
/// 姝ゅ嚱鏁扮敤浜庨噴鏀剧敱 Rust 鍒嗛厤鐨勫唴瀛橈紝闇€瑕佺‘淇濅紶鍏ョ殑鎸囬拡鏄敱 Rust 鍒嗛厤鐨?/// 浣跨敤 catch_unwind 闃叉 panic 瀵艰嚧 JVM 宕╂簝
#[no_mangle]
pub unsafe extern "C" fn free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    
    let _ = std::panic::catch_unwind(|| {
        let _ = CString::from_raw(ptr);
    });
}

/// 灏?C 瀛楃涓茶浆鎹负 Rust 瀛楃涓?fn c_str_to_string(ptr: *const c_char) -> Result<String, std::str::Utf8Error> {
    if ptr.is_null() {
        return Ok(String::new());
    }
    
    unsafe {
        let c_str = CStr::from_ptr(ptr);
        c_str.to_str().map(|s| s.to_string())
    }
}

