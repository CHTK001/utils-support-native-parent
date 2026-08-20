//! Rust HTTP 桥接库
//!
//! Java 通过 Panama FFM 以动态库方式加载本库（cdylib），
//! Java 是 HTTP 入口（{@code ShmHttpServer}），Rust hyper 负责真正的 HTTP 监听与响应。
//! 两者通过共享内存环形队列（req/resp 两条队列）通信。

mod queue;
mod server;

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use server::{Bridge, ThreadHandles};

#[cfg(windows)]
mod winsock;

/// 轮询/读取单次最大阻塞时间（纳秒），用于让线程能感知停止信号
pub const MAX_POLL_WAIT_NS: u64 = 200_000_000;

/// 请求信封头部长度：req_id(8) + method_len(2) + path_len(2) + body_len(4)
pub const REQ_HEADER_LEN: usize = 16;

/// 请求 ID 自增器
static REQ_ID: AtomicU64 = AtomicU64::new(1);

/// 是否已启动
static STARTED: AtomicBool = AtomicBool::new(false);

/// 运行中的桥接状态
static BRIDGE: Mutex<Option<Arc<Bridge>>> = Mutex::new(None);

/// 工作线程句柄
static THREADS: Mutex<Option<ThreadHandles>> = Mutex::new(None);

/// 生成下一个请求 ID
pub fn next_req_id() -> u64 {
    REQ_ID.fetch_add(1, Ordering::Relaxed)
}

/// 规范化共享内存对象名（Windows 文件映射名不允许斜杠）
#[cfg(windows)]
fn sanitize_name(name: &str) -> String {
    name.replace(['/', '\\'], "_")
}

#[cfg(not(windows))]
fn sanitize_name(name: &str) -> String {
    name.to_string()
}

/// 启动 HTTP 桥接。
///
/// # Safety
/// `shm_name` 必须指向有效的以 NUL 结尾的 UTF-8 字符串。
#[no_mangle]
pub unsafe extern "C" fn rhb_start(
    port: u32,
    shm_name: *const c_char,
    capacity: u32,
    slot_size: u32,
) -> i32 {
    if STARTED.swap(true, Ordering::AcqRel) {
        return -15; // already started
    }
    if shm_name.is_null() || capacity < 2 || slot_size < 16 {
        STARTED.store(false, Ordering::Release);
        return -1;
    }
    let name = match CStr::from_ptr(shm_name).to_str() {
        Ok(s) => sanitize_name(s),
        Err(_) => {
            STARTED.store(false, Ordering::Release);
            return -1;
        }
    };
    let req_name = format!("{name}_req");
    let resp_name = format!("{name}_resp");

    let req_queue = match queue::Queue::create(&req_name, capacity, slot_size) {
        Ok(q) => q,
        Err(rc) => {
            STARTED.store(false, Ordering::Release);
            return rc;
        }
    };
    let resp_queue = match queue::Queue::create(&resp_name, capacity, slot_size) {
        Ok(q) => q,
        Err(rc) => {
            STARTED.store(false, Ordering::Release);
            return rc;
        }
    };

    #[cfg(windows)]
    if let Err(rc) = winsock::init() {
        STARTED.store(false, Ordering::Release);
        return rc;
    }

    let running = Arc::new(AtomicBool::new(true));
    let bridge = Arc::new(Bridge {
        req_queue,
        resp_queue,
        slot_size,
        running: running.clone(),
        pending: Arc::new(Mutex::new(std::collections::HashMap::new())),
    });

    let hyper_bridge = bridge.clone();
    let hyper_thread = std::thread::Builder::new()
        .name("rhb-hyper".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("[rhb] tokio build failed: {e}");
                    return;
                }
            };
            rt.block_on(server::run_hyper(port as u16, hyper_bridge));
        })
        .expect("spawn hyper thread");

    let resp_bridge = bridge.clone();
    let resp_thread = std::thread::Builder::new()
        .name("rhb-resp".into())
        .spawn(move || server::resp_reader_loop(resp_bridge))
        .expect("spawn resp thread");

    *BRIDGE.lock().unwrap() = Some(bridge);
    *THREADS.lock().unwrap() = Some(ThreadHandles {
        hyper: hyper_thread,
        resp: resp_thread,
    });
    0
}

/// 停止 HTTP 桥接，回收线程与队列。
#[no_mangle]
pub extern "C" fn rhb_stop() -> i32 {
    let bridge = BRIDGE.lock().unwrap().take();
    if let Some(b) = bridge {
        b.running.store(false, Ordering::Release);
        // 清空 pending，让正在等待响应的请求立刻失败退出，避免 join 阻塞
        b.pending.lock().unwrap().clear();
        let threads = THREADS.lock().unwrap().take();
        if let Some(t) = threads {
            let _ = t.hyper.join();
            let _ = t.resp.join();
        }
    }
    STARTED.store(false, Ordering::Release);
    0
}

/// 从请求队列取出一条请求信封（阻塞至多 MAX_POLL_WAIT_NS）。
///
/// 返回 >0：写入 out 的字节数；返回 0：无数据（超时）；返回 <0：错误。
///
/// # Safety
/// `out` 必须指向至少 `out_cap` 字节的可写内存。
#[no_mangle]
pub unsafe extern "C" fn rhb_poll_request(out: *mut u8, out_cap: u32) -> i32 {
    let bridge = match BRIDGE.lock().unwrap().as_ref() {
        Some(b) => b.clone(),
        None => return -15,
    };
    if out.is_null() || out_cap == 0 {
        return -1;
    }
    let buf = std::slice::from_raw_parts_mut(out, out_cap as usize);
    server::poll_request(&bridge, buf)
}

/// 向响应队列写入一条响应信封。
///
/// # Safety
/// `ct`/`body` 必须指向至少 `ct_len`/`body_len` 字节的可读内存（可为 NULL 且长度 0）。
#[no_mangle]
pub unsafe extern "C" fn rhb_send_response(
    req_id: u64,
    status: u16,
    ct: *const u8,
    ct_len: u32,
    body: *const u8,
    body_len: u32,
) -> i32 {
    let bridge = match BRIDGE.lock().unwrap().as_ref() {
        Some(b) => b.clone(),
        None => return -15,
    };
    let ct_bytes: &[u8] = if ct.is_null() || ct_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ct, ct_len as usize)
    };
    let body_bytes: &[u8] = if body.is_null() || body_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(body, body_len as usize)
    };
    server::send_response(&bridge, req_id, status, ct_bytes, body_bytes)
}
