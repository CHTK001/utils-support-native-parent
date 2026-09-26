//! `uia_rust` 原生库自测：验证 COM 初始化、桌面根元素遍历与控件树导出。
//!
//! 用法：
//! - 无参数：打印桌面根元素下前若干层控件树；
//! - `"<标题子串>"`：绑定该标题窗口并打印其控件树；
//! - `"<标题子串>" <maxDepth> <maxNodes>`：指定深度与节点上限。

use std::process::ExitCode;

use uia_rust::{
    RC_OK, error::uia_last_error, uia_attach_window, uia_create, uia_describe, uia_destroy,
    uia_dump_tree, uia_find, uia_free_string, uia_release_all,
};

/// 读取最近一次错误描述。
fn last_error() -> String {
    let p = uia_last_error();
    if p.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

/// 取出原生返回的字符串指针并释放原生内存。
fn take(p: *mut std::ffi::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    unsafe { uia_free_string(p) };
    s
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let title = args.get(1).cloned();
    let depth: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let nodes: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);

    let mut handle: i64 = 0;
    let rc = unsafe { uia_create(&mut handle) };
    if rc != RC_OK || handle == 0 {
        eprintln!("[FAIL] uia_create rc={rc} err={}", last_error());
        return ExitCode::FAILURE;
    }
    println!("[OK]   uia_create handle={handle}");

    if let Some(t) = title.as_deref() {
        let needle = std::ffi::CString::new(t).unwrap();
        let mut hwnd: i64 = 0;
        let rc = unsafe { uia_attach_window(handle, needle.as_ptr(), std::ptr::null(), 1, &mut hwnd) };
        if rc != RC_OK {
            eprintln!("[FAIL] uia_attach_window({t}) rc={rc} err={}", last_error());
            unsafe { uia_destroy(handle) };
            return ExitCode::FAILURE;
        }
        println!("[OK]   绑定窗口 hwnd={hwnd:#x}");
    }

    // 找一个 Edit 控件，验证选择器引擎与属性读取通路
    // 注意：入参必须是 NUL 结尾的 C 字符串，故用 CString 而非字节串字面量
    let sel = std::ffi::CString::new(r#"{"controlType":"Edit"}"#).unwrap();
    let mut ids = [0i64; 32];
    let mut count: i32 = 0;
    let rc = unsafe {
        uia_find(
            handle,
            sel.as_ptr(),
            ids.as_mut_ptr(),
            ids.len() as i32,
            &mut count,
        )
    };
    if rc != RC_OK {
        eprintln!("[WARN] uia_find 失败: {}", last_error());
    } else {
        println!("[OK]   uia_find Edit 命中 {count} 个");
        if count > 0 {
            let n = count.min(ids.len() as i32) as usize;
            let ids_json = format!("{:?}", &ids[..n]);
            let mut out: *mut std::ffi::c_char = std::ptr::null_mut();
            let rc = unsafe {
                uia_describe(handle, std::ffi::CString::new(ids_json).unwrap().as_ptr(), &mut out)
            };
            if rc == RC_OK {
                let text = take(out);
                println!("[OK]   uia_describe: {} 字节", text.len());
            } else {
                eprintln!("[WARN] uia_describe 失败: {}", last_error());
            }
        }
    }
    unsafe { uia_release_all(handle) };

    let mut out: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = unsafe { uia_dump_tree(handle, depth, nodes, &mut out) };
    if rc != RC_OK {
        eprintln!("[FAIL] uia_dump_tree rc={rc} err={}", last_error());
        unsafe { uia_destroy(handle) };
        return ExitCode::FAILURE;
    }
    let tree = take(out);    println!("[OK]   uia_dump_tree 输出 {} 字节", tree.len());
    println!("{}", &tree[..tree.len().min(2000)]);

    unsafe { uia_destroy(handle) };
    println!("[OK]   uia_destroy 完成");
    ExitCode::SUCCESS
}
