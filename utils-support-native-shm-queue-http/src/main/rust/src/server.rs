//! hyper HTTP 服务器 + 请求/响应队列桥接。
//! 单环 Zero-copy：请求和响应共享同一块 SHM，同一槽位 REQ→RESP 原地复用。

use crate::{BRIDGE, Channel, REQ_HEADER_LEN};
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// 响应信封头部长度：req_id(8) + status(2) + ct_len(2) + body_len(4)
const RESP_HEADER_LEN: usize = 16;

/// 响应体上限（超限返回 413）
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// 等待 Java 业务处理的超时
const WAIT_HANDLER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 单个响应的载荷
pub struct RespData {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// 桥接全局状态
pub struct Bridge {
    /// 请求通道
    pub req_channel: Channel,
    /// 响应通道
    pub resp_channel: Channel,
    /// 槽大小
    pub slot_size: u32,
    /// 运行标志
    pub running: Arc<std::sync::atomic::AtomicBool>,
    /// req_id -> oneshot Sender，供响应回填
    pub pending: Arc<Mutex<HashMap<u64, oneshot::Sender<RespData>>>>,
    /// 上一次轮询到的 req 槽位（用于 send_response 写入相同槽位）
    pub last_polled_req_slot: std::sync::atomic::AtomicU32,
}

/// 桥接线程句柄
pub struct ThreadHandles {
    pub hyper: std::thread::JoinHandle<()>,
    pub resp: std::thread::JoinHandle<()>,
}

/// 构建请求信封
pub fn build_req_envelope(req_id: u64, method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(crate::REQ_HEADER_LEN + method.len() + path.len() + body.len());
    v.extend_from_slice(&req_id.to_le_bytes());
    v.extend_from_slice(&(method.len() as u16).to_le_bytes());
    v.extend_from_slice(&(path.len() as u16).to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(method.as_bytes());
    v.extend_from_slice(path.as_bytes());
    v.extend_from_slice(body);
    v
}

/// 从请求通道取出一条请求信封（阻塞至多 MAX_POLL_WAIT_NS）。
///
/// 返回：写入 out 的字节数；
/// 返回 0：无数据（超时）；
/// 返回 <0：错误。
/// 成功时会在 bridge.last_polled_req_slot 记录槽索引，供 send_response 使用。
pub fn poll_request(bridge: &Bridge, out: &mut [u8]) -> i32 {
    match bridge.req_channel.poll_req_timeout(crate::MAX_POLL_WAIT_NS) {
        Ok((slot, ptr, len)) => {
            // 将数据复制到 out buffer
            let copy_len = std::cmp::min(len, out.len() as u32);
            // 记录槽索引，供 send_response 写入相同槽位
            bridge.last_polled_req_slot.store(slot, Ordering::Relaxed);
            unsafe {
                std::ptr::copy_nonoverlapping(ptr, out.as_mut_ptr(), copy_len as usize);
            }
            copy_len as i32
        }
        Err(rc) => {
            // 即使出错也尝试保存槽索引
            let slot = bridge.last_polled_req_slot.load(Ordering::Relaxed);
            bridge.last_polled_req_slot.store(slot, Ordering::Relaxed);
            rc
        }
    }
}

/// Java 写响应通道——在上一次轮询到的槽位上原地覆写响应（zero-copy）。
///
/// # Safety
/// `ct`/`body` 必须指向至少 `ct_len`/`body_len` 字节的可读内存（可为 NULL 且长度 0）。
pub fn send_response(
    bridge: &Bridge,
    req_id: u64,
    status: u16,
    ct: &[u8],
    body: &[u8],
) -> i32 {
    let mut v = Vec::with_capacity(RESP_HEADER_LEN + ct.len() + body.len());
    v.extend_from_slice(&req_id.to_le_bytes());
    v.extend_from_slice(&status.to_le_bytes());
    v.extend_from_slice(&(ct.len() as u16).to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(ct);
    v.extend_from_slice(body);

    // 在上一次 poll_request 到的槽位上原地写入响应（zero-copy）
    let slot = bridge.last_polled_req_slot.load(Ordering::Relaxed);
    // 写入：[4字节 RESP 状态] [4字节 body_len] [body...]
    // 注意：write_resp_direct 内部会处理状态写入和 commit
    match bridge.resp_channel.write_resp_direct(slot, &v) {
        Ok(()) => 0,
        Err(rc) => {
            eprintln!("[rhb] write_resp_direct 失败 rc={} slot={}", rc, slot);
            rc
        }
    }
}

/// hyper 服务主循环（在 tokio current_thread runtime 上运行）
pub async fn run_hyper(port: u16, bridge: Arc<Bridge>) {
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[rhb] bind {addr} failed: {e}");
            return;
        }
    };
    eprintln!("[rhb] hyper listening on {addr}");
    loop {
        if !bridge.running.load(Ordering::Relaxed) {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => {
                        // 关闭 Nagle：HTTP 小包场景下 Nagle+delayed-ACK 会引入 ~10ms 固定延迟
                        let _ = stream.set_nodelay(true);
                        let b = bridge.clone();
                        tokio::spawn(async move {
                            handle_conn(stream, b).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("[rhb] accept error: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

/// 处理单个 TCP 连接
async fn handle_conn(stream: tokio::net::TcpStream, bridge: Arc<Bridge>) {
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;

    let io = TokioIo::new(stream);
    let service = service_fn(move |req| handle_req(req, bridge.clone()));
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(io, service)
        .await;
}

/// 处理单个 HTTP 请求
async fn handle_req(
    req: hyper::Request<hyper::body::Incoming>,
    bridge: Arc<Bridge>,
) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>, std::convert::Infallible> {
    let method = req.method().as_str().to_string();
    // 保留查询串，供 Java 侧解析 query 参数（path 字段按 path?query 传递）
    let path = match req.uri().query() {
        Some(q) => format!("{}?{}", req.uri().path(), q),
        None => req.uri().path().to_string(),
    };
    let (_, body) = req.into_parts();
    let body_bytes = match collect_body(body).await {
        Ok(b) => b,
        Err(_) => return Ok(json_resp(413, "request body too large")),
    };

    let req_id = crate::next_req_id();
    let envelope = build_req_envelope(req_id, &method, &path, &body_bytes);

    // 1. 获取空槽，写入请求数据，标记为 REQ
    let (slot, ptr) = match bridge.req_channel.acquire_empty() {
        Ok(s) => s,
        Err(rc) => {
            bridge.pending.lock().unwrap().remove(&req_id);
            eprintln!("[rhb] acquire_empty failed: {rc}");
            return Ok(json_resp(503, "server busy"));
        }
    };
    // 复制 enrolle 数据到槽ptr
    // 槽前 4 字节由 C 端 commit_req 写入长度头，数据从 ptr+4 开始
    unsafe {
        std::ptr::copy_nonoverlapping(envelope.as_ptr(), ptr.add(4), envelope.len());
    }
    // 标记为 REQ
    if let Err(rc) = bridge.req_channel.commit_req(slot, envelope.len() as u32) {
        bridge.pending.lock().unwrap().remove(&req_id);
        eprintln!("[rhb] commit_req failed: {rc}");
        return Ok(json_resp(503, "server busy"));
    }

    // 2. 注册 pending，等待响应
    let (tx, rx) = oneshot::channel();
    {
        let mut guard = bridge.pending.lock().unwrap();
        guard.insert(req_id, tx);
    }

    // 3. 等待响应超时
    let resp_data = match tokio::time::timeout(WAIT_HANDLER_TIMEOUT, rx).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => return Ok(json_resp(503, "server closed")),
        Err(_) => {
            eprintln!("[rhb] handler timeout for req_id={}", req_id);
            // 清理 pending 并回收槽
            bridge.req_channel.release(slot);
            bridge.pending.lock().unwrap().remove(&req_id);
            return Ok(json_resp(504, "handler timeout"));
        }
    };

    // 从 RespData 构建 hyper::Response
    let status = hyper::StatusCode::from_u16(resp_data.status)
        .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = hyper::Response::new(http_body_util::Full::new(bytes::Bytes::from(resp_data.body)));
    *response.status_mut() = status;
    if !resp_data.content_type.is_empty() {
        if let Ok(v) = resp_data.content_type.parse::<hyper::header::HeaderValue>() {
            response.headers_mut().insert(hyper::header::CONTENT_TYPE, v);
        }
    }

    Ok(response)
}

/// 收集请求体，超过 MAX_BODY_BYTES 返回 Err
async fn collect_body(mut body: hyper::body::Incoming) -> Result<bytes::Bytes, ()> {
    use http_body_util::BodyExt;
    let mut buf = bytes::BytesMut::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| ())?;
        if let Some(data) = frame.data_ref() {
            buf.extend_from_slice(data);
            if buf.len() > MAX_BODY_BYTES {
                return Err(());
            }
        }
    }
    Ok(buf.freeze())
}

/// 构造 JSON 错误响应
fn json_resp(status: u16, msg: &str) -> hyper::Response<http_body_util::Full<bytes::Bytes>> {
    let body = format!("{{\"error\":\"{}\"}}", msg);
    let mut response = hyper::Response::new(http_body_util::Full::new(bytes::Bytes::from(body)));
    *response.status_mut() = hyper::StatusCode::from_u16(status).unwrap_or_default();
    response
        .headers_mut()
        .insert(hyper::header::CONTENT_TYPE, "application/json".parse().unwrap());
    response
}

/// resp_reader_loop：轮询响应通道，回填 pending sender。
pub fn resp_reader_loop(bridge: Arc<Bridge>) {
    while bridge.running.load(Ordering::Relaxed) {
        match poll_and_complete_resp(&bridge) {
            Some(_) => {}
            None => {
                // 没有数据，短暂休眠避免空转
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

/// 从响应通道轮询 RESP 槽，回填 pending，释放槽。
fn poll_and_complete_resp(bridge: &Bridge) -> Option<RespData> {
    // 从响应通道轮询 RESP
    let (slot, ptr, len) = match bridge.resp_channel.poll_resp_timeout(crate::MAX_POLL_WAIT_NS) {
        Ok(s) => s,
        Err(rc) => {
            return None;
        }
    };
    // 将原始指针转换为字节切片以便安全读取
    let data = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    if len < RESP_HEADER_LEN as u32 {
        eprintln!("[rhb] resp drop: n={} < header", len);
        // 即使太短也释放槽，避免死锁
        bridge.resp_channel.release(slot);
        return None;
    }
    let req_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let status = u16::from_le_bytes(data[8..10].try_into().unwrap());
    let ct_len = u16::from_le_bytes(data[10..12].try_into().unwrap()) as usize;
    let body_len = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    if RESP_HEADER_LEN + ct_len + body_len > len as usize {
        eprintln!("[rhb] resp len mismatch n={} need={}", len, RESP_HEADER_LEN + ct_len + body_len);
        bridge.resp_channel.release(slot);
        return None;
    }
    let ct = String::from_utf8_lossy(&data[16..16 + ct_len]).into_owned();
    let body = data[16 + ct_len..16 + ct_len + body_len].to_vec();
    // 释放槽回 EMPTY
    bridge.resp_channel.release(slot);
    // 回填 pending
    if let Some(tx) = bridge.pending.lock().unwrap().remove(&req_id) {
        let _ = tx.send(RespData {
            status,
            content_type: ct,
            body,
        });
    } else {
        eprintln!("[rhb] resp drop: no pending for req_id={}", req_id);
    }
    // 返回给调用者（主循环可选择性使用，这里主要是为了完成 pending）
    None
}