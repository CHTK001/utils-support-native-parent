use std::ffi::{c_char, c_int, CStr, CString};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use smb_server::{Access, LocalFsBackend, Share, SmbServer};

/// SMB 服务器句柄。
/// 持有 tokio Runtime 和停止信号。
struct SmbServerHandle {
    _rt: tokio::runtime::Runtime,
    stop_flag: Arc<AtomicBool>,
    share_name: String,
    root_path: String,
}

/// 启动 SMB 服务器。
///
/// # 参数
/// - `bind_addr`: 绑定地址 C 字符串 (如 "0.0.0.0")
/// - `port`: 监听端口
/// - `share_name`: 共享名称 C 字符串
/// - `root_path`: 本地根路径 C 字符串
/// - `user`: 用户名 C 字符串 (可选，空表示匿名)
/// - `password`: 密码 C 字符串 (可选)
///
/// # 返回
/// 成功返回堆句柄的地址 (isize > 0)，失败返回负数错误码。
#[no_mangle]
pub extern "C" fn smb_server_start(
    bind_addr: *const c_char,
    port: c_int,
    share_name: *const c_char,
    root_path: *const c_char,
    user: *const c_char,
    password: *const c_char,
) -> isize {
    if bind_addr.is_null() || share_name.is_null() || root_path.is_null() {
        return -1;
    }

    let bind_addr_str = unsafe { CStr::from_ptr(bind_addr) }.to_string_lossy().to_string();
    let share_name = unsafe { CStr::from_ptr(share_name) }.to_string_lossy().to_string();
    let root_path = unsafe { CStr::from_ptr(root_path) }.to_string_lossy().to_string();
    let port = if port > 0 && port <= 65535 {
        port as u16
    } else {
        445
    };

    let user = if user.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(user) }.to_string_lossy().to_string()
    };
    let password = if password.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(password) }.to_string_lossy().to_string()
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return -2,
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = Arc::clone(&stop_flag);

    let share_name_clone = share_name.clone();
    let root_path_clone = root_path.clone();
    let user_clone = user.clone();
    let password_clone = password.clone();

    rt.spawn(async move {
        let addr: SocketAddr = match format!("{}:{}", bind_addr_str, port).parse() {
            Ok(addr) => addr,
            Err(e) => {
                eprintln!("[rust_smb_server] Invalid address: {}", e);
                return;
            }
        };

        let backend = match LocalFsBackend::new(&root_path_clone) {
            Ok(backend) => backend,
            Err(e) => {
                eprintln!("[rust_smb_server] Failed to create backend: {}", e);
                return;
            }
        };

        let mut share = Share::new(&share_name_clone, backend);
        if user_clone.is_empty() {
            // Empty user means anonymous access is intended.
            share = share.public();
        } else {
            share = share.user(&user_clone, Access::ReadWrite);
        } else {
            // 无用户配置时启用公共模式，允许匿名访问
            share = share.public();
        }

        let mut builder = SmbServer::builder()
            .listen(addr)
            .share(share);

        if !user_clone.is_empty() {
            builder = builder.user(&user_clone, &password_clone);
        }

        let server = match builder.build() {
            Ok(srv) => srv,
            Err(e) => {
                eprintln!("[rust_smb_server] Failed to build server: {}", e);
                return;
            }
        };

        // 使用 select! 允许通过 stop_flag 优雅关闭
        tokio::select! {
            result = server.serve() => {
                if let Err(e) = result {
                    eprintln!("[rust_smb_server] Server error: {}", e);
                }
            }
            _ = async {
                while !stop_flag_clone.load(Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            } => {}
        }
    });

    let handle = Box::new(SmbServerHandle {
        _rt: rt,
        stop_flag,
        share_name,
        root_path,
    });

    Box::into_raw(handle) as isize
}

/// 停止 SMB 服务器。
///
/// # 参数
/// - `handle_ptr`: smb_server_start 返回的句柄地址
///
/// # 返回
/// 0 表示成功，负数表示失败。
#[no_mangle]
pub extern "C" fn smb_server_stop(handle_ptr: isize) -> c_int {
    if handle_ptr <= 0 {
        return -1;
    }

    let handle = unsafe { Box::from_raw(handle_ptr as *mut SmbServerHandle) };

    handle.stop_flag.store(true, Ordering::SeqCst);

    // 给正在处理的请求一个短暂的优雅关闭窗口
    std::thread::sleep(std::time::Duration::from_millis(100));

    drop(handle);
    0
}

/// 列出所有共享名称。
/// 返回以 \0 分隔、以 \0\0 结尾的 C 字符串。
/// 调用方必须使用 smb_server_free_string 释放返回的内存。
///
/// # 参数
/// - `handle_ptr`: smb_server_start 返回的句柄地址
///
/// # 返回
/// C 字符串指针，NULL 表示错误。
#[no_mangle]
pub extern "C" fn smb_server_list_shares(handle_ptr: isize) -> *const c_char {
    if handle_ptr <= 0 {
        return std::ptr::null();
    }

    let handle = unsafe { &*(handle_ptr as *const SmbServerHandle) };

    // 当前版本只返回配置的 share_name
    let mut result = handle.share_name.clone();
    result.push('\0');
    result.push('\0');

    match CString::new(result) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null(),
    }
}

/// 释放 smb_server_list_shares 返回的字符串。
///
/// # 参数
/// - `s`: smb_server_list_shares 返回的 C 字符串指针
#[no_mangle]
pub extern "C" fn smb_server_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(s);
    }
}