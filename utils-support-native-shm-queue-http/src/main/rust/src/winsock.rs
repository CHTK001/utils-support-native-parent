//! Windows Winsock 初始化。
//!
//! 进程内未调用 WSAStartup 时，Winsock 返回错误 10091 (WSASYSNOTREADY)，
//! 导致 hyper bind 失败。在加载 hyper 之前必须初始化。

#[cfg(windows)]
use windows_sys::Win32::Networking::WinSock::{WSAStartup, WSADATA};

/// 初始化 Winsock（仅 Windows）。已初始化时重复调用无害。
#[cfg(windows)]
pub fn init() -> Result<(), i32> {
    unsafe {
        let mut data: WSADATA = std::mem::zeroed();
        // MAKEWORD(2,2) = 0x0202，请求 2.2 版本；返回 0 表示成功
        let rc = WSAStartup(0x0202, &mut data);
        if rc != 0 {
            return Err(rc as i32);
        }
    }
    Ok(())
}
