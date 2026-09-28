//! 线程局部错误信息，镜像 `wechat_wcdb` 的 `wechat_wcdb_last_error` 语义。

use std::cell::RefCell;
use std::ffi::{c_char, CString};

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// 记录最近一次错误（线程局部）。
///
/// # 参数
///
/// - `msg`：错误描述，内部 NUL 会被替换为空格以保证可转为 C 字符串。
pub fn set_error(msg: &str) {
    let sanitized = msg.replace('\0', " ");
    let c = CString::new(sanitized).unwrap_or_default();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(c));
}

/// 记录错误并返回统一的失败返回码。
///
/// # 参数
///
/// - `msg`：错误描述。
pub fn fail(msg: &str) -> i32 {
    set_error(msg);
    crate::RC_FAIL
}

/// 清空最近一次错误。
pub fn clear_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// 取出最近一次错误的 C 字符串指针（由线程局部存储持有，调用方不得释放）。
///
/// # 返回
///
/// 错误描述指针；无错误时返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn uia_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| match slot.borrow().as_ref() {
        Some(s) => s.as_ptr(),
        None => std::ptr::null(),
    })
}
