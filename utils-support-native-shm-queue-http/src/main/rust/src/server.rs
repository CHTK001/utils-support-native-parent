//! hyper HTTP 服务器 + 请求/响应队列桥接。

use crate::channel::Channel;
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
    /// req_id -> oneshot Sender，供响应读取线程回填
    pub pending: Arc<Mutex<HashMap<u64, oneshot::Sender<RespData>>>>,
}

/// 工作线程句柄
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

/// 从响应通道读取并回填 pending
pub fn resp_reader_loop(bridge: Arc<Bridge>) {
    let mut buf = vec![0u8; bridge.slot_size as usize];
    while bridge.running.load(Ordering::Relaxed) {
        let n = match bridge.resp_channel.recv_timeout(&mut buf, crate::MAX_POLL_WAIT_NS) {
            Ok(n) => n,
            Err(rc) if rc == -12 => continue,
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
        };
        if n < RESP_HEADER_LEN {
            eprintln!("[rhb] resp drop: n={} < header", n);
            continue;
        }
        let req_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let status = u16::from_le_bytes(buf[8..10].try_into().unwrap());
        let ct_len = u16::from_le_bytes(buf[10..12].try_into().unwrap()) as usize;
        let body_len = u32::from_le_bytes(buf[12..16].try_into().unwrap()) as usize;
        if RESP_HEADER_LEN + ct_len + body_len > n {
            eprintln!("[rhb] resp drop: len mismatch n={} need={}", n, RESP_HEADER_LEN + ct_len + body_len);
            continue;
        }
        let ct = String::from_utf8_lossy(&buf[16..16 + ct_len]).into_owned();
        let body = buf[16 + ct_len..16 + ct_len + body_len].to_vec();
        if let Some(tx) = bridge.pending.lock().unwrap().remove(&req_id) {
            let _ = tx.send(RespData {
                status,
                content_type: ct,
                body,
            });
        } else {
            eprintln!("[rhb] resp drop: no pending for req_id={}", req_id);
        }
    }
}

/// Java 轮询请求通道，返回信封字节数（0=超时无数据，负数=错误）
pub fn poll_request(bridge: &Bridge, out: &mut [u8]) -> i32 {
    match bridge.req_channel.recv_timeout(out, crate::MAX_POLL_WAIT_NS) {
        Ok(n) => n as i32,
        Err(rc) if rc == -12 => 0,
        Err(rc) => rc,
    }
}

/// Java 写响应通道
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
    match bridge.resp_channel.send(&v) {
        Ok(()) => 0,
        Err(rc) => rc,
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

    // 先登记 pending 再入队，避免响应早于 pending 注册而丢失
    let (tx, rx) = oneshot::channel();
    {
        let mut guard = bridge.pending.lock().unwrap();
        guard.insert(req_id, tx);
    }
    if let Err(rc) = bridge.req_channel.send(&envelope) {
        bridge.pending.lock().unwrap().remove(&req_id);
        eprintln!("[rhb] req queue send failed: {rc}");
        return Ok(json_resp(503, "server busy"));
    }

    let resp = match tokio::time::timeout(WAIT_HANDLER_TIMEOUT, rx).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => return Ok(json_resp(503, "server closed")),
        Err(_) => {
            eprintln!("[rhb] handler timeout for req_id={}", req_id);
            return Ok(json_resp(504, "handler timeout"));
        }
    };

    let status = hyper::StatusCode::from_u16(resp.status)
        .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = hyper::Response::new(http_body_util::Full::new(bytes::Bytes::from(resp.body)));
    *response.status_mut() = status;
    if !resp.content_type.is_empty() {
        if let Ok(v) = resp.content_type.parse::<hyper::header::HeaderValue>() {
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
