//! macOS 平台实现（`libproc` + `sysctl` + 少量命令行工具）。
//!
//! # 契约
//! 本文件只需实现一个入口：`call(op, args) -> JSON`，返回值必须是
//! `crate::model::Envelope` 的序列化结果。**不支持的能力必须返回
//! `Envelope { ok: false, error: ... }`**（用 [`unsupported`] 生成），
//! 不要返回空集合——"不支持"与"支持但为空"调用方必须能区分。
//!
//! # 本文件的边界
//! 只放**平台专属**逻辑。三平台一致的字段请走 `crate::common`，
//! 不要复制一份。
//!
//! # macOS 的能力约束（务必先读）
//! macOS 是三平台里能力最弱的：
//!
//! * `libproc`（`proc_listpids` / `proc_pidinfo` / `proc_pidpath` / `proc_pidfdinfo`）
//!   对**同 uid 或 root** 可读，这是本文件的主力接口；
//! * 读取**其他进程**的镜像/内存映射/栈需要 `task_for_pid`，受 SIP 与代码签名限制，
//!   普通签名进程拿不到，因此这些能力对非自身进程一律显式返回不支持；
//! * 事件订阅需要 EndpointSecurity 框架的 Apple 授权 entitlement，本模块拿不到，
//!   **不会用轮询伪装成事件**；
//! * 硬件传感器需要访问 SMC（IOKit 私有接口），无公开 API。
//!
//! 拿不到的能力一律返回"不支持 + 具体原因"，绝不返回编造的 0 或空集合。
//!
//! # 参数与返回
//! `args` 是 JSON 对象字符串，用 `serde_json` 解析；缺字段一律按 `None` 处理，
//! 不允许 panic。所有对外错误都转成 `Envelope::err`，不 panic 跨 FFI 边界。

use crate::common;
use crate::model::{
    unsupported, ActionKind, ActionResult, CredentialInfo, DiskIo, Envelope, EnvVar, GpuInfo,
    HandleInfo, KernelModuleInfo, MappingInfo, MemoryModule, ModuleInfo, ProcessDetail,
    ServiceInfo, SocketInfo, ThreadInfo,
};
use crate::PLATFORM;

use std::ffi::CStr;
use std::io::Error;
use std::process::{self, Command};
use std::ptr;

use serde::Serialize;
use serde_json::Value;

/// 序列化失败时的兜底响应：不以空串或空指针返回，避免调用方分不清"失败"与"空结果"。
const FALLBACK_ERR: &str = r#"{"ok":false,"data":null,"error":"结果序列化失败"}"#;

// ============================================================================
// 0. 通用小工具
// ============================================================================

/// 构造成功信封的 JSON。
///
/// # 参数
/// * `data` - 任意可序列化的结果
///
/// # 返回值
/// `Envelope::ok(data)` 的 JSON 串；序列化失败时返回兜底错误 JSON
fn ok_json<T: Serialize>(data: T) -> String {
    match serde_json::to_string(&Envelope::ok(data)) {
        Ok(s) => s,
        Err(_) => FALLBACK_ERR.to_string(),
    }
}

/// 构造失败信封的 JSON。
///
/// # 参数
/// * `error` - 失败原因，任意可显示类型
///
/// # 返回值
/// `Envelope::err(error)` 的 JSON 串
fn err_json<E: std::fmt::Display>(error: E) -> String {
    match serde_json::to_string(&Envelope::<()>::err(error)) {
        Ok(s) => s,
        Err(_) => FALLBACK_ERR.to_string(),
    }
}

/// 标准"平台不支持"错误，附带具体原因。
///
/// # 参数
/// * `op` - 操作名
/// * `reason` - 具体原因（需 root / 需 task_for_pid / 需 entitlement 等）
///
/// # 返回值
/// 失败信封的 JSON 串
fn unsupported_reason(op: &str, reason: &str) -> String {
    err_json(format!("{}；原因: {}", unsupported(op, PLATFORM), reason))
}

/// 解析 args JSON。空串或非法 JSON 一律按"无参数"处理，不 panic。
///
/// # 参数
/// * `args` - JSON 对象字符串，可为空
///
/// # 返回值
/// 解析出的 `Value`；失败时为 `Value::Null`
fn parse_args(args: &str) -> Value {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or(Value::Null)
}

/// 从参数对象里取整数，兼容数字与数字字符串两种写法。
///
/// # 参数
/// * `args` - 参数对象
/// * `keys` - 依次尝试的键名
///
/// # 返回值
/// 首个命中的整数值
fn arg_i64(args: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        match args.get(*key) {
            Some(Value::Number(n)) => {
                if let Some(v) = n.as_i64() {
                    return Some(v);
                }
                if let Some(v) = n.as_u64() {
                    return i64::try_from(v).ok();
                }
                if let Some(v) = n.as_f64() {
                    return Some(v as i64);
                }
            }
            Some(Value::String(s)) => {
                if let Ok(v) = s.trim().parse::<i64>() {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

/// 从参数对象里取字符串。数字会被转成其十进制文本。
///
/// # 参数
/// * `args` - 参数对象
/// * `keys` - 依次尝试的键名
///
/// # 返回值
/// 首个命中的字符串；空串视为未提供
fn arg_str(args: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        match args.get(*key) {
            Some(Value::String(s)) => {
                if !s.trim().is_empty() {
                    return Some(s.clone());
                }
            }
            Some(Value::Number(n)) => {
                return Some(n.to_string());
            }
            _ => {}
        }
    }
    None
}

/// 从参数对象里取布尔值，兼容 `true/false/1/0`。
///
/// # 参数
/// * `args` - 参数对象
/// * `keys` - 依次尝试的键名
///
/// # 返回值
/// 首个命中的布尔值
fn arg_bool(args: &Value, keys: &[&str]) -> Option<bool> {
    for key in keys {
        match args.get(*key) {
            Some(Value::Bool(b)) => {
                return Some(*b);
            }
            Some(Value::String(s)) => {
                let lower = s.trim().to_ascii_lowercase();
                if lower == "true" || lower == "1" {
                    return Some(true);
                }
                if lower == "false" || lower == "0" {
                    return Some(false);
                }
            }
            _ => {}
        }
    }
    None
}

/// 取必填的 `pid` 参数。
///
/// # 参数
/// * `args` - 参数对象
///
/// # 返回值
/// 进程 ID；缺失或超出 i32 范围时返回错误描述
fn require_pid(args: &Value) -> Result<i32, String> {
    match arg_i64(args, &["pid"]) {
        Some(v) => i32::try_from(v).map_err(|_| format!("pid 超出 i32 范围: {}", v)),
        None => Err("缺少参数 pid".to_string()),
    }
}

/// 最近一次系统调用的 errno 文本。
///
/// # 返回值
/// errno 的可读描述
fn last_errno() -> String {
    Error::last_os_error().to_string()
}

/// 读小端/本机序 u32（本机序，与内核写入一致）。
///
/// # 参数
/// * `buf` - 原始字节缓冲
/// * `off` - 偏移
///
/// # 返回值
/// 越界时返回 `None`
fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    if off + 4 > buf.len() {
        return None;
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[off..off + 4]);
    Some(u32::from_ne_bytes(b))
}

/// 读本机序 u64。
///
/// # 参数
/// * `buf` - 原始字节缓冲
/// * `off` - 偏移
///
/// # 返回值
/// 越界时返回 `None`
fn read_u64(buf: &[u8], off: usize) -> Option<u64> {
    if off + 8 > buf.len() {
        return None;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[off..off + 8]);
    Some(u64::from_ne_bytes(b))
}

/// 从字节缓冲的指定偏移读 NUL 结尾字符串。
///
/// # 参数
/// * `buf` - 原始字节缓冲
/// * `off` - 起始偏移
/// * `max` - 最多看多少字节
///
/// # 返回值
/// 去掉首尾空白的字符串；空白串或越界返回 `None`
fn read_cstr(buf: &[u8], off: usize, max: usize) -> Option<String> {
    if off >= buf.len() {
        return None;
    }
    let end = std::cmp::min(off.saturating_add(max), buf.len());
    let slice = &buf[off..end];
    let nul = slice.iter().position(|c| *c == 0).unwrap_or(slice.len());
    let text = String::from_utf8_lossy(&slice[..nul]).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 找到与 `open` 处 `{` 配对的 `}` 的下标。
///
/// # 参数
/// * `text` - 文本
/// * `open` - `{` 的下标
///
/// # 返回值
/// 配对括号下标；不配对时返回 `None`
fn find_matching_brace(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if open >= bytes.len() || bytes[open] != b'{' {
        return None;
    }
    let mut depth = 0i32;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        if *b == b'{' {
            depth += 1;
        } else if *b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// 在 `ioreg` 的属性字典文本里取数值属性。
///
/// `ioreg` 的字典形如 `{"Bytes (Read)"=1234,"In use system memory"=5678}`，
/// 这里容错解析，不依赖固定顺序与空白。
///
/// # 参数
/// * `dict` - 花括号内部的文本（不含最外层花括号）
/// * `key` - 属性名（不含引号）
///
/// # 返回值
/// 解析出的数值
fn dict_f64(dict: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\"", key);
    let mut from = 0usize;
    while let Some(rel) = dict[from..].find(&needle) {
        let after_key = from + rel + needle.len();
        let rest = &dict[after_key..];
        let trimmed = rest.trim_start();
        if let Some(eq_rest) = trimmed.strip_prefix('=') {
            let value = eq_rest.trim_start();
            let end = value
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
                .unwrap_or(value.len());
            if end > 0 {
                if let Ok(v) = value[..end].parse::<f64>() {
                    return Some(v);
                }
            }
        }
        from = after_key;
    }
    None
}

// ============================================================================
// 1. 命令行工具
// ============================================================================

/// 执行外部命令并要求退出码为 0。
///
/// # 参数
/// * `prog` - 可执行文件绝对路径
/// * `args` - 参数数组
///
/// # 返回值
/// 标准输出文本；启动失败或非 0 退出时返回错误描述
fn run_command(prog: &str, args: &[&str]) -> Result<String, String> {
    let output = run_command_raw(prog, args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let code = match output.status.code() {
            Some(c) => c.to_string(),
            None => "被信号终止".to_string(),
        };
        Err(format!(
            "命令 {} 退出码 {}: {}",
            prog,
            code,
            stderr_snippet(&output.stderr)
        ))
    }
}

/// 执行外部命令，只要求能启动，不检查退出码。
///
/// `lsof` 这类工具在"无匹配结果"时也返回 1，此时输出为空属于**正常空结果**而非失败，
/// 因此需要与 [`run_command`] 区分。
///
/// # 参数
/// * `prog` - 可执行文件绝对路径
/// * `args` - 参数数组
///
/// # 返回值
/// 命令输出；无法启动（不存在/无执行权限）时返回错误描述
fn run_command_raw(prog: &str, args: &[&str]) -> Result<process::Output, String> {
    Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| format!("无法执行命令 {}: {}", prog, e))
}

/// 截取 stderr 首行片段，避免把整屏错误塞进 JSON。
///
/// # 参数
/// * `stderr` - 原始 stderr 字节
///
/// # 返回值
/// 最多 200 字符的首行文本
fn stderr_snippet(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text.lines().next().unwrap_or("").trim().to_string();
    if first.chars().count() > 200 {
        first.chars().take(200).collect::<String>() + "..."
    } else {
        first
    }
}

// ============================================================================
// 2. libproc：进程 BSD 信息
// ============================================================================

/// `struct proc_bsdinfo`（PROC_PIDTBSDINFO）里本实现关心的字段。
///
/// 只保留稳定可用的部分；结构体尾部字段在历史版本里是**追加**的，因此用偏移 +
/// 长度校验的方式读取，字段缺失时返回 `None` 而不是读出垃圾。
struct BsdInfo {
    /// 父进程 ID。
    ppid: i32,
    /// 有效用户 ID（`pbi_uid`）。
    uid: u32,
    /// 有效组 ID（`pbi_gid`）。
    gid: u32,
    /// 真实用户 ID。
    ruid: u32,
    /// 真实组 ID。
    rgid: u32,
    /// 进程状态码（SIDL=1 / SRUN=2 / SSLEEP=3 / SSTOP=4 / SZOMB=5）。
    status: u32,
    /// nice 值。
    nice: i32,
    /// 已打开文件数（含 socket/pipe 等 fd）。
    nfiles: u32,
    /// 启动秒。
    start_sec: u64,
    /// 启动微秒。
    start_usec: u64,
    /// 短命令名（16 字节）。
    comm: Option<String>,
}

/// 读取进程的 BSD 信息。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 解析出的信息；进程不存在或无权限时返回错误描述
fn bsd_info(pid: i32) -> Result<BsdInfo, String> {
    let mut buf = [0u8; 256];
    let rc = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as i32,
        )
    };
    if rc <= 0 {
        return Err(proc_err(pid, "PROC_PIDTBSDINFO"));
    }
    Ok(parse_bsd_info(&buf[..rc as usize]))
}

/// 按 `struct proc_bsdinfo` 的固定偏移解析字节缓冲。
///
/// # 参数
/// * `buf` - `proc_pidinfo` 写入的缓冲（长度以返回值为准）
///
/// # 返回值
/// 解析结果，缺字段时用安全默认值（`0` / `None`）
fn parse_bsd_info(buf: &[u8]) -> BsdInfo {
    BsdInfo {
        ppid: read_u32(buf, 16).map(|v| v as i32).unwrap_or(0),
        uid: read_u32(buf, 20).unwrap_or(0),
        gid: read_u32(buf, 24).unwrap_or(0),
        ruid: read_u32(buf, 28).unwrap_or(0),
        rgid: read_u32(buf, 32).unwrap_or(0),
        status: read_u32(buf, 4).unwrap_or(0),
        nice: read_u32(buf, 116).map(|v| v as i32).unwrap_or(0),
        nfiles: read_u32(buf, 96).unwrap_or(0),
        start_sec: read_u64(buf, 120).unwrap_or(0),
        start_usec: read_u64(buf, 128).unwrap_or(0),
        // pbi_comm 位于偏移 48，长度 MAXCOMLEN=16
        comm: read_cstr(buf, 48, libc::MAXCOMLEN),
    }
}

/// 把 BSD 状态码翻译成模型里的状态串。
///
/// # 参数
/// * `code` - `pbi_status`
///
/// # 返回值
/// running / sleeping / stopped / zombie / idle / unknown
fn status_str(code: u32) -> String {
    match code {
        1 => "idle".to_string(),
        2 => "running".to_string(),
        3 => "sleeping".to_string(),
        4 => "stopped".to_string(),
        5 => "zombie".to_string(),
        _ => "unknown".to_string(),
    }
}

/// 由 nice 值给出调度优先级类别。
///
/// macOS 的实时调度类（SCHED_FIFO/SCHED_RR）需 `task_for_pid` 才能读他进程，
/// 这里只按 nice 给出相对高低，不冒充实时类。
///
/// # 参数
/// * `nice` - nice 值
///
/// # 返回值
/// high / low / normal
fn priority_class(nice: i32) -> String {
    if nice < 0 {
        "high".to_string()
    } else if nice > 0 {
        "low".to_string()
    } else {
        "normal".to_string()
    }
}

/// 由 BSD 启动时间字段计算 Unix 毫秒。
///
/// # 参数
/// * `sec` - 启动秒
/// * `usec` - 启动微秒
///
/// # 返回值
/// 毫秒时间戳；`sec` 为 0（struct 尾部字段缺失）时返回 `None`
fn start_time_ms(sec: u64, usec: u64) -> Option<i64> {
    if sec == 0 {
        return None;
    }
    Some((sec as i64) * 1000 + (usec as i64) / 1000)
}

/// 取进程会话 ID（Unix 语义的 session）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 会话 ID；进程不存在时返回 `None`
fn session_id_of(pid: i32) -> Option<u32> {
    let sid = unsafe { libc::getsid(pid) };
    if sid < 0 {
        None
    } else {
        u32::try_from(sid).ok()
    }
}

/// 统一的 libproc 调用失败描述，按 errno 给出可执行的原因。
///
/// # 参数
/// * `pid` - 目标进程
/// * `what` - 失败的调用名
///
/// # 返回值
/// 失败描述
fn proc_err(pid: i32, what: &str) -> String {
    match Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => format!("进程 {} 不存在", pid),
        Some(libc::EPERM) => {
            format!("读取进程 {} 的 {} 需要与目标同 uid 或 root 权限", pid, what)
        }
        _ => format!("读取进程 {} 的 {} 失败: {}", pid, what, last_errno()),
    }
}

/// 取进程可执行文件路径（`proc_pidpath`）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 可执行文件路径；取不到返回 `None`
fn pid_path(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let rc = unsafe {
        libc::proc_pidpath(
            pid,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if rc <= 0 {
        return None;
    }
    read_cstr(&buf, 0, buf.len())
}

/// 用 libproc 补齐一条进程记录的 macOS 可读字段。
///
/// 补齐 `ppid` / `uid` / `gid` / `status` / `priority` / `priority_class` /
/// `session_id` / `handle_count` / `start_time_ms`；读不到的保持原值，不写死 0。
///
/// # 参数
/// * `p` - 待补齐的进程详情，原地修改
fn enrich_process(p: &mut ProcessDetail) {
    if let Ok(b) = bsd_info(p.pid) {
        if b.ppid > 0 {
            p.ppid = Some(b.ppid);
        }
        if let Ok(uid) = i32::try_from(b.uid) {
            p.uid = Some(uid);
        }
        if let Ok(gid) = i32::try_from(b.gid) {
            p.gid = Some(gid);
        }
        p.status = status_str(b.status);
        p.priority = Some(b.nice);
        p.priority_class = Some(priority_class(b.nice));
        if let Some(ms) = start_time_ms(b.start_sec, b.start_usec) {
            p.start_time_ms = Some(ms);
        }
        if b.nfiles > 0 {
            p.handle_count = Some(b.nfiles);
        }
        if p.name.is_empty() {
            if let Some(comm) = b.comm.clone() {
                p.name = comm;
            }
        }
        if p.user.is_none() {
            p.user = common::user_name(b.uid);
        }
    }
    if p.exe_path.is_none() {
        p.exe_path = pid_path(p.pid);
    }
    p.session_id = session_id_of(p.pid);
}

// ============================================================================
// 3. process.* 实现
// ============================================================================

/// `process.list`：在 sysinfo 基线上补齐 macOS 可读字段。
///
/// # 返回值
/// 进程详情列表的 JSON 信封
fn op_process_list() -> String {
    let mut list = common::sysinfo_processes();
    for p in list.iter_mut() {
        enrich_process(p);
    }
    ok_json(list)
}

/// `process.detail`：按 pid 返回单个进程详情。
///
/// # 参数
/// * `args` - 需含 `pid`
///
/// # 返回值
/// 进程详情 JSON 信封
fn op_process_detail(args: &Value) -> String {
    let pid = match require_pid(args) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    let mut list = common::sysinfo_processes();
    match list.iter().position(|p| p.pid == pid) {
        Some(idx) => {
            let mut detail = list.remove(idx);
            enrich_process(&mut detail);
            ok_json(detail)
        }
        None => err_json(format!(
            "进程 {} 不在进程列表内（可能已退出或对当前用户不可见）",
            pid
        )),
    }
}

/// 列出进程的 fd 表（PROC_PIDLISTFDS）。
///
/// `proc_pidinfo` 在缓冲区为 NULL 时返回所需字节数；缓冲区被打满时翻倍重试。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// `(fd, fdtype)` 列表；进程不存在或无权限时返回错误描述
fn list_fds(pid: i32) -> Result<Vec<(i32, u32)>, String> {
    let need = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            ptr::null_mut(),
            0,
        )
    };
    let mut cap = if need > 0 { need as usize + 64 } else { 4096 };
    for _ in 0..3 {
        let mut buf = vec![0u8; cap];
        let rc = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len() as i32,
            )
        };
        if rc < 0 {
            return Err(proc_err(pid, "PROC_PIDLISTFDS"));
        }
        let used = rc as usize;
        if used == 0 {
            return Ok(Vec::new());
        }
        if used < buf.len() {
            return Ok(parse_fd_list(&buf[..used]));
        }
        cap = cap.saturating_mul(2);
    }
    Err(format!("进程 {} 的 fd 列表过大，三次扩容后仍未读全", pid))
}

/// 解析 `PROC_PIDLISTFDS` 返回的 `struct proc_fdinfo` 数组（每项 8 字节）。
///
/// # 参数
/// * `bytes` - 原始缓冲
///
/// # 返回值
/// `(fd, fdtype)` 列表
fn parse_fd_list(bytes: &[u8]) -> Vec<(i32, u32)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= bytes.len() {
        let fd = read_u32(bytes, off).map(|v| v as i32).unwrap_or(-1);
        let fdtype = read_u32(bytes, off + 4).unwrap_or(u32::MAX);
        if fd >= 0 {
            out.push((fd, fdtype));
        }
        off += 8;
    }
    out
}

/// 读取单个 vnode fd 的路径（PROC_PIDFDVNODEPATHINFO）。
///
/// 结构为 `struct vnode_fdinfo`（32 字节）后跟 `char pvip_path[MAXPATHLEN]`。
///
/// # 参数
/// * `pid` - 进程 ID
/// * `fd` - fd 号
///
/// # 返回值
/// `(是否目录, 路径)`；读不到时返回 `(false, None)`
fn vnode_path(pid: i32, fd: i32) -> (bool, Option<String>) {
    /// `PROC_PIDFDVNODEPATHINFO`
    const PROC_PIDFDVNODEPATHINFO: i32 = 2;
    /// vnode 路径结构：32 字节定长头 + MAXPATHLEN(1024)
    const VNODE_BUF: usize = 32 + 1024;
    let mut buf = vec![0u8; VNODE_BUF];
    let rc = unsafe {
        libc::proc_pidfdinfo(
            pid,
            fd,
            PROC_PIDFDVNODEPATHINFO,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as i32,
        )
    };
    if rc <= 0 {
        return (false, None);
    }
    // pvi_type 位于偏移 28，VDIR=2
    let is_dir = read_u32(&buf, 28) == Some(2);
    (is_dir, read_cstr(&buf, 32, 1024))
}

/// 读取单个 fd 的打开标志（`struct proc_fileinfo.fi_open_flags`）。
///
/// # 参数
/// * `pid` - 进程 ID
/// * `fd` - fd 号
///
/// # 返回值
/// 以 `0x` 开头的十六进制标志串
fn fd_access(pid: i32, fd: i32) -> Option<String> {
    /// `PROC_PIDFDVNODEINFO`
    const PROC_PIDFDVNODEINFO: i32 = 1;
    /// `struct vnode_fdinfo` 定长部分
    const VNODE_HDR: usize = 32;
    let mut buf = vec![0u8; VNODE_HDR];
    let rc = unsafe {
        libc::proc_pidfdinfo(
            pid,
            fd,
            PROC_PIDFDVNODEINFO,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as i32,
        )
    };
    if rc <= 0 {
        return None;
    }
    read_u32(&buf, 0).map(|flags| format!("0x{:x}", flags))
}

/// 把 fd 类型翻译成模型里的 kind，并尽量取到目标名。
///
/// # 参数
/// * `pid` - 进程 ID
/// * `fd` - fd 号
/// * `fdtype` - `PROX_FDTYPE_*`
///
/// # 返回值
/// `(kind, name)`
fn fd_kind_and_name(pid: i32, fd: i32, fdtype: u32) -> (String, Option<String>) {
    match fdtype as i32 {
        libc::PROX_FDTYPE_VNODE => {
            let (is_dir, path) = vnode_path(pid, fd);
            if is_dir {
                ("directory".to_string(), path)
            } else {
                ("file".to_string(), path)
            }
        }
        libc::PROX_FDTYPE_SOCKET => ("socket".to_string(), None),
        libc::PROX_FDTYPE_PIPE => ("pipe".to_string(), None),
        libc::PROX_FDTYPE_KQUEUE => ("event".to_string(), None),
        libc::PROX_FDTYPE_FSEVENTS => ("event".to_string(), None),
        libc::PROX_FDTYPE_PSHM => ("unknown".to_string(), None),
        libc::PROX_FDTYPE_PSEM => ("unknown".to_string(), None),
        libc::PROX_FDTYPE_ATALK => ("socket".to_string(), None),
        libc::PROX_FDTYPE_NETPOLICY => ("unknown".to_string(), None),
        libc::PROX_FDTYPE_CHANNEL => ("unknown".to_string(), None),
        libc::PROX_FDTYPE_NEXUS => ("unknown".to_string(), None),
        _ => ("unknown".to_string(), None),
    }
}

/// `process.handles`：列出进程 fd（对同 uid 或 root 可用，无需 task_for_pid）。
///
/// # 参数
/// * `args` - 需含 `pid`
///
/// # 返回值
/// [`HandleInfo`] 列表的 JSON 信封
fn op_process_handles(args: &Value) -> String {
    let pid = match require_pid(args) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    let fds = match list_fds(pid) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    let mut out = Vec::with_capacity(fds.len());
    for (fd, fdtype) in fds {
        let (kind, name) = fd_kind_and_name(pid, fd, fdtype);
        out.push(HandleInfo {
            id: fd.to_string(),
            kind,
            name,
            access: fd_access(pid, fd),
            ref_count: None,
        });
    }
    ok_json(out)
}

/// `process.credential`：进程凭据。
///
/// macOS 没有 Windows 的令牌概念，`token_type` / `impersonation_level` /
/// `privileges` 一律为空；`elevated` 以"有效 uid 是否为 0（root）"表达。
///
/// # 参数
/// * `args` - 需含 `pid`
///
/// # 返回值
/// [`CredentialInfo`] 的 JSON 信封
fn op_process_credential(args: &Value) -> String {
    let pid = match require_pid(args) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    let b = match bsd_info(pid) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    let credentials = CredentialInfo {
        owner: common::user_name(b.uid),
        // macOS 无令牌：下列字段没有对应语义，保持空而不是硬填
        token_type: None,
        impersonation_level: None,
        integrity_level: None,
        elevated: Some(b.uid == 0),
        // 进程附加组需 task_for_pid 才能枚举，不伪造
        groups: Vec::new(),
        privileges: Vec::new(),
        capabilities: Vec::new(),
        seccomp: None,
        no_new_privs: None,
        security_label: None,
        // entitlements 需 codesign 解析目标签名，且对他进程多为不可读，不伪造
        entitlements: Vec::new(),
    };
    // ruid/rgid 仅用于校验读取成功，不进模型（模型无真实 uid 字段）
    let _ = (b.ruid, b.rgid);
    ok_json(credentials)
}

/// 列出本进程已加载的镜像（`_dyld_*`）。
///
/// # 返回值
/// 模块列表
///
/// `libc` 把 `_dyld_*` 系列标为 deprecated 并建议 `mach2` crate，但本模块依赖里
/// 没有 `mach2`（`Cargo.toml` 不允许改），故沿用 `libc` 的稳定声明。
#[allow(deprecated)]
fn self_modules() -> Vec<ModuleInfo> {
    let count = unsafe { libc::_dyld_image_count() };
    let mut out = Vec::new();
    let mut i = 0u32;
    while i < count {
        let name_ptr = unsafe { libc::_dyld_get_image_name(i) };
        let path = if name_ptr.is_null() {
            None
        } else {
            let cstr = unsafe { CStr::from_ptr(name_ptr) };
            Some(cstr.to_string_lossy().into_owned())
        };
        let header = unsafe { libc::_dyld_get_image_header(i) };
        let base = header as u64;
        let name = match path.as_deref() {
            Some(p) => p.rsplit('/').next().unwrap_or("").to_string(),
            None => String::new(),
        };
        out.push(ModuleInfo {
            name,
            path,
            base_address: if base == 0 {
                None
            } else {
                Some(format!("0x{:x}", base))
            },
            size: None,
            version: None,
            company: None,
            description: None,
            signature: None,
        });
        i += 1;
    }
    out
}

/// `process.modules`：已加载模块。
///
/// 仅本进程可用 `_dyld_image_count`；其他进程需
/// `task_for_pid` + `task_info(TASK_DYLD_INFO)` + `mach_vm_read` 读远端
/// `dyld_all_image_infos`，受 SIP 与代码签名限制。
///
/// # 参数
/// * `args` - 可选 `pid`，缺省为本进程
///
/// # 返回值
/// [`ModuleInfo`] 列表的 JSON 信封
fn op_process_modules(args: &Value) -> String {
    let self_pid = process::id() as i32;
    let pid = arg_i64(args, &["pid"])
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(self_pid);
    if pid != self_pid {
        return unsupported_reason(
            "process.modules",
            "读取其他进程已加载镜像需 task_for_pid + task_info(TASK_DYLD_INFO) + mach_vm_read，\
             受 SIP 与代码签名限制对普通进程不可用；仅支持查询本进程（_dyld_image_count）",
        );
    }
    ok_json(self_modules())
}

/// 列出进程线程 ID（PROC_PIDLISTTHREADS）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 线程信息列表；每项的详细信息（优先级/CPU 时间）需 task_for_pid，故留空
fn threads_of(pid: i32) -> Result<Vec<ThreadInfo>, String> {
    /// `PROC_PIDLISTTHREADS`
    const PROC_PIDLISTTHREADS: i32 = 6;
    let need = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDLISTTHREADS,
            0,
            ptr::null_mut(),
            0,
        )
    };
    if need <= 0 {
        return Err(proc_err(pid, "PROC_PIDLISTTHREADS"));
    }
    let mut buf = vec![0u8; need as usize + 64];
    let rc = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDLISTTHREADS,
            0,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as i32,
        )
    };
    if rc <= 0 {
        return Err(proc_err(pid, "PROC_PIDLISTTHREADS"));
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= rc as usize {
        // 64 位系统上 PROC_PIDLISTTHREADS 每项为 uint64 的 thread id
        let tid = read_u64(&buf, off).unwrap_or(0);
        if tid != 0 {
            out.push(ThreadInfo {
                tid: tid as i64,
                pid,
                status: None,
                priority: None,
                user_time_ms: None,
                kernel_time_ms: None,
                start_address: None,
                stack_base: None,
                wait_reason: None,
                name: None,
            });
        }
        off += 8;
    }
    Ok(out)
}

/// `process.threads`：线程列表。
///
/// # 参数
/// * `args` - 需含 `pid`
///
/// # 返回值
/// [`ThreadInfo`] 列表的 JSON 信封
fn op_process_threads(args: &Value) -> String {
    let pid = match require_pid(args) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    match threads_of(pid) {
        Ok(v) => ok_json(v),
        Err(e) => err_json(e),
    }
}

/// 跳过缓冲中的一条 NUL 结尾字符串，返回下一条的起点。
///
/// # 参数
/// * `buf` - 缓冲
/// * `from` - 起始偏移
///
/// # 返回值
/// 下一个偏移；`from` 越界时返回缓冲长度
fn skip_cstr(buf: &[u8], from: usize) -> usize {
    if from >= buf.len() {
        return buf.len();
    }
    match buf[from..].iter().position(|c| *c == 0) {
        Some(p) => from + p + 1,
        None => buf.len(),
    }
}

/// 解析 `sysctl KERN_PROCARGS2` 的结果，提取环境变量。
///
/// 布局：`int argc` → 可执行路径（NUL 结尾）→ 若干对齐 NUL → `argc` 个
/// argv（NUL 结尾）→ 环境变量（NUL 结尾，以空串收尾）。
///
/// # 参数
/// * `buf` - 内核写入的缓冲
///
/// # 返回值
/// 环境变量列表；布局不符时返回空列表而不是乱报
fn parse_procargs2(buf: &[u8]) -> Vec<EnvVar> {
    if buf.len() < 4 {
        return Vec::new();
    }
    let argc = read_u32(buf, 0).unwrap_or(0) as usize;
    let mut off = 4usize;
    // 可执行路径
    off = skip_cstr(buf, off);
    // 对齐用的连续 NUL
    while off < buf.len() && buf[off] == 0 {
        off += 1;
    }
    // argv
    for _ in 0..argc {
        if off >= buf.len() {
            return Vec::new();
        }
        off = skip_cstr(buf, off);
    }
    let mut out = Vec::new();
    while off < buf.len() {
        if buf[off] == 0 {
            break;
        }
        let end = match buf[off..].iter().position(|c| *c == 0) {
            Some(p) => off + p,
            None => buf.len(),
        };
        let text = String::from_utf8_lossy(&buf[off..end]).into_owned();
        if let Some(eq) = text.find('=') {
            if eq > 0 {
                out.push(EnvVar {
                    key: text[..eq].to_string(),
                    value: text[eq + 1..].to_string(),
                });
            }
        }
        off = end + 1;
    }
    out
}

/// 统一的 `sysctl` 失败描述。
///
/// # 参数
/// * `pid` - 目标进程
/// * `what` - 失败的调用名
///
/// # 返回值
/// 失败描述
fn env_err(pid: i32, what: &str) -> String {
    match Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => format!(
            "读取进程 {} 的 {} 需要 root 权限（非 root 只能读取自身 uid 的进程）",
            pid, what
        ),
        Some(libc::ESRCH) => format!("进程 {} 不存在", pid),
        _ => format!("读取进程 {} 的 {} 失败: {}", pid, what, last_errno()),
    }
}

/// 读取进程环境变量（`sysctl KERN_PROCARGS2`）。
///
/// # 参数
/// * `pid` - 进程 ID
///
/// # 返回值
/// 环境变量列表；无权限时返回错误描述
fn env_of(pid: i32) -> Result<Vec<EnvVar>, String> {
    let mut argmax: libc::c_int = 0;
    let mut argmax_len = std::mem::size_of::<libc::c_int>();
    let mut mib_argmax = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let rc = unsafe {
        libc::sysctl(
            mib_argmax.as_mut_ptr(),
            mib_argmax.len() as u32,
            &mut argmax as *mut libc::c_int as *mut libc::c_void,
            &mut argmax_len,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || argmax <= 0 {
        return Err(format!("读取内核参数 KERN_ARGMAX 失败: {}", last_errno()));
    }
    let mut buf = vec![0u8; argmax as usize];
    let mut buf_len = buf.len();
    let mut mib_args = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let rc2 = unsafe {
        libc::sysctl(
            mib_args.as_mut_ptr(),
            mib_args.len() as u32,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut buf_len,
            ptr::null_mut(),
            0,
        )
    };
    if rc2 != 0 {
        return Err(env_err(pid, "KERN_PROCARGS2"));
    }
    if buf_len > buf.len() {
        buf_len = buf.len();
    }
    Ok(parse_procargs2(&buf[..buf_len]))
}

/// `process.env`：环境变量。
///
/// # 参数
/// * `args` - 需含 `pid`
///
/// # 返回值
/// [`EnvVar`] 列表的 JSON 信封
fn op_process_env(args: &Value) -> String {
    let pid = match require_pid(args) {
        Ok(v) => v,
        Err(e) => {
            return err_json(e);
        }
    };
    match env_of(pid) {
        Ok(v) => ok_json(v),
        Err(e) => err_json(e),
    }
}

/// 读取本进程的内存映射（`mach_vm_region`）。
///
/// 只对本进程可行：`mach_vm_region` 需要目标 task port，他进程走 `task_for_pid`。
/// 返回的 `info` 是 `natural_t` 数组，前三个 natural 依次为
/// protection / max_protection / inheritance。
///
/// # 返回值
/// 映射列表；首个 region 就失败时返回错误描述
///
/// `libc::mach_task_self` 被标为 deprecated（建议 `mach2` crate），但依赖里没有
/// `mach2`，故沿用 `libc` 声明。
#[allow(deprecated)]
fn self_mappings() -> Result<Vec<MappingInfo>, String> {
    /// `VM_REGION_BASIC_INFO_64`
    const VM_REGION_BASIC_INFO_64: i32 = 9;
    /// `KERN_SUCCESS`
    const KERN_SUCCESS: i32 = 0;
    /// `KERN_INVALID_ADDRESS`：区域遍历的正常结束标志
    const KERN_INVALID_ADDRESS: i32 = 1;
    /// 单个进程最多收集的区域数，防御性上限，避免异常时死循环
    const MAX_REGIONS: usize = 200_000;

    let task = unsafe { libc::mach_task_self() };
    let pid = process::id() as i32;
    let mut out: Vec<MappingInfo> = Vec::new();
    let mut address: u64 = 0;
    loop {
        let mut size: u64 = 0;
        let mut info = [0i32; 12];
        let mut count: u32 = info.len() as u32;
        let mut object_name: u32 = 0;
        let kr = unsafe {
            mach_vm_region(
                task,
                &mut address,
                &mut size,
                VM_REGION_BASIC_INFO_64,
                info.as_mut_ptr(),
                &mut count,
                &mut object_name,
            )
        };
        if object_name != 0 {
            unsafe {
                mach_port_deallocate(task, object_name);
            }
        }
        if kr == KERN_INVALID_ADDRESS {
            break;
        }
        if kr != KERN_SUCCESS {
            if out.is_empty() {
                return Err(format!("mach_vm_region 失败（kern_return={}）", kr));
            }
            break;
        }
        let protection = info.first().copied().unwrap_or(0);
        let path = region_path(pid, address);
        out.push(MappingInfo {
            base_address: format!("0x{:x}", address),
            size,
            protection: prot_str(protection),
            kind: mapping_kind(path.as_deref(), protection),
            path,
        });
        if size == 0 || out.len() >= MAX_REGIONS {
            break;
        }
        address = address.wrapping_add(size);
    }
    Ok(out)
}

/// 取包含指定地址的映射对应的文件路径（`proc_regionfilename`）。
///
/// # 参数
/// * `pid` - 进程 ID
/// * `address` - 区域内地址
///
/// # 返回值
/// 文件路径；匿名映射返回 `None`
fn region_path(pid: i32, address: u64) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let rc = unsafe {
        libc::proc_regionfilename(
            pid,
            address,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if rc <= 0 {
        return None;
    }
    read_cstr(&buf, 0, buf.len())
}

/// 把 `vm_prot_t` 掩码转成 `rwx` 三字符串。
///
/// # 参数
/// * `prot` - 权限掩码（VM_PROT_READ=1 / WRITE=2 / EXECUTE=4）
///
/// # 返回值
/// 形如 `r-x` 的串
fn prot_str(prot: i32) -> String {
    let read = if prot & 1 != 0 { 'r' } else { '-' };
    let write = if prot & 2 != 0 { 'w' } else { '-' };
    let execute = if prot & 4 != 0 { 'x' } else { '-' };
    format!("{}{}{}", read, write, execute)
}

/// 推断映射类别。
///
/// # 参数
/// * `path` - 映射文件路径
/// * `prot` - 权限掩码
///
/// # 返回值
/// image / mapped / private / stack / heap / unknown
fn mapping_kind(path: Option<&str>, prot: i32) -> String {
    match path {
        Some(p) if p.starts_with('/') => {
            if p.contains(".dylib") || p.starts_with("/usr/lib") || p.starts_with("/System") {
                "image".to_string()
            } else {
                "mapped".to_string()
            }
        }
        Some(_) => "mapped".to_string(),
        None => {
            // 无路径的匿名区域：无执行权限时按私有数据区归类
            if prot & 4 == 0 {
                "private".to_string()
            } else {
                "unknown".to_string()
            }
        }
    }
}

/// `process.mappings`：内存映射。
///
/// # 参数
/// * `args` - 可选 `pid`，缺省为本进程
///
/// # 返回值
/// [`MappingInfo`] 列表的 JSON 信封
fn op_process_mappings(args: &Value) -> String {
    let self_pid = process::id() as i32;
    let pid = match arg_i64(args, &["pid"]) {
        Some(v) => match i32::try_from(v) {
            Ok(p) => p,
            Err(_) => {
                return err_json(format!("pid 超出 i32 范围: {}", v));
            }
        },
        None => self_pid,
    };
    if pid != self_pid {
        return unsupported_reason(
            "process.mappings",
            "读取其他进程的内存映射需 task_for_pid（受 SIP 与代码签名限制）；\
             仅支持查询本进程（mach_vm_region）",
        );
    }
    match self_mappings() {
        Ok(v) => ok_json(v),
        Err(e) => err_json(e),
    }
}

/// `process.stack`：栈回溯。
///
/// 本模块不做远程内存读取，因此他进程用户态栈与内核态栈都返回不支持。
///
/// # 参数
/// * `args` - 含 `pid` / `tid` / `kernel`
///
/// # 返回值
/// 一律为失败信封（附带具体原因）
fn op_process_stack(args: &Value) -> String {
    let kernel = arg_bool(args, &["kernel"]).unwrap_or(false);
    if kernel {
        return unsupported_reason(
            "process.stack",
            "内核态栈需通过内核扩展（kext）或内核调试器获取，用户态库无此能力",
        );
    }
    unsupported_reason(
        "process.stack",
        "读取其他进程的用户态栈需 task_for_pid（受 SIP 与代码签名限制）并对目标内存做 \
         mach_vm_read；本模块不实现远程栈回溯，也不返回伪造帧",
    )
}

// ============================================================================
// 4. 系统级能力
// ============================================================================

/// 解析 `ioreg` 输出里的 `"Statistics"` 或 `"PerformanceStatistics"` 字典。
///
/// 用"属性名 → 最近的 `+-o <节点名>`"关联归属，避免多行字典打断行解析。
///
/// # 参数
/// * `text` - `ioreg` 的标准输出
/// * `property` - 属性名（不含引号）
/// * `wanted` - 需要读取的属性键列表
///
/// # 返回值
/// `(节点名, 键值对列表)` 的结果集
fn parse_ioreg_dicts(text: &str, property: &str, wanted: &[&str]) -> Vec<(String, Vec<(String, f64)>)> {
    let needle = format!("\"{}\"", property);
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(&needle) {
        let pos = from + rel;
        let node = match text[..pos].rfind("+-o ") {
            Some(p) => {
                let rest = &text[p + 4..];
                rest.split_whitespace().next().unwrap_or("").to_string()
            }
            None => String::new(),
        };
        let mut values: Vec<(String, f64)> = Vec::new();
        if let Some(open_rel) = text[pos..].find('{') {
            let open = pos + open_rel;
            if let Some(close) = find_matching_brace(text, open) {
                let dict = &text[open + 1..close];
                for key in wanted {
                    if let Some(v) = dict_f64(dict, key) {
                        values.push(((*key).to_string(), v));
                    }
                }
            }
        }
        if !node.is_empty() && !values.is_empty() {
            out.push((node, values));
        }
        from = pos + needle.len();
        if from >= text.len() {
            break;
        }
    }
    out
}

/// 在键值对列表里按键名取数值。
///
/// # 参数
/// * `values` - 键值对列表
/// * `key` - 键名
///
/// # 返回值
/// 命中的数值
fn pick(values: &[(String, f64)], key: &str) -> Option<f64> {
    values
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| *v)
}

/// 读取每块磁盘的累计 IO（IOKit `IOBlockStorageDriver/Statistics`）。
///
/// `iostat` 只输出速率，没有累计字节数，因此这里走 IOKit 属性。
///
/// # 返回值
/// 磁盘 IO 列表；无法解析时返回空列表（由调用方转成明确的不支持说明）
fn disks_io() -> Result<Vec<DiskIo>, String> {
    let text = run_command(
        "/usr/sbin/ioreg",
        &[
            "-r",
            "-c",
            "IOBlockStorageDriver",
            "-k",
            "Statistics",
            "-w",
            "0",
        ],
    )?;
    let wanted = [
        "Bytes (Read)",
        "Bytes (Write)",
        "Operations (Read)",
        "Operations (Write)",
    ];
    let parsed = parse_ioreg_dicts(&text, "Statistics", &wanted);
    let mut out: Vec<DiskIo> = Vec::new();
    for (node, values) in parsed {
        if out.iter().any(|d| d.name == node) {
            continue;
        }
        let read_bytes = pick(&values, "Bytes (Read)").map(|v| v as u64);
        let written_bytes = pick(&values, "Bytes (Write)").map(|v| v as u64);
        if read_bytes.is_none() && written_bytes.is_none() {
            continue;
        }
        out.push(DiskIo {
            name: node,
            read_bytes: read_bytes.unwrap_or(0),
            written_bytes: written_bytes.unwrap_or(0),
            read_count: pick(&values, "Operations (Read)").map(|v| v as u64).unwrap_or(0),
            write_count: pick(&values, "Operations (Write)").map(|v| v as u64).unwrap_or(0),
            queue_depth: None,
        });
    }
    Ok(out)
}

/// `disk.io`：每磁盘累计 IO。
///
/// # 返回值
/// [`DiskIo`] 列表的 JSON 信封
fn op_disk_io() -> String {
    match disks_io() {
        Ok(v) if !v.is_empty() => ok_json(v),
        Ok(_) => err_json(
            "无法从 IOKit 读取块设备统计：ioreg 未返回 IOBlockStorageDriver 的 Statistics 字典",
        ),
        Err(e) => err_json(e),
    }
}

/// 根据加速器节点名推断 GPU 厂商。
///
/// # 参数
/// * `node` - IOKit 节点名
///
/// # 返回值
/// apple / amd / intel / nvidia / unknown
fn gpu_vendor(node: &str) -> String {
    let upper = node.to_ascii_uppercase();
    if upper.contains("AGX") || upper.contains("APPLE") {
        "apple".to_string()
    } else if upper.contains("AMD") || upper.contains("RADEON") {
        "amd".to_string()
    } else if upper.contains("GEFORCE") || upper.contains("NVIDIA") {
        "nvidia".to_string()
    } else if upper.contains("INTEL") {
        "intel".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 读取 GPU 统计（IOKit `IOAccelerator/PerformanceStatistics`）。
///
/// # 返回值
/// GPU 列表；无法解析时返回空列表
fn gpus() -> Result<Vec<GpuInfo>, String> {
    let text = run_command(
        "/usr/sbin/ioreg",
        &["-r", "-c", "IOAccelerator", "-w", "0"],
    )?;
    let wanted = [
        "Device Utilization %",
        "In use system memory",
        "Alloc system memory",
        "Temperature(C)",
    ];
    let parsed = parse_ioreg_dicts(&text, "PerformanceStatistics", &wanted);
    let mut out: Vec<GpuInfo> = Vec::new();
    for (node, values) in parsed {
        if out.iter().any(|g| g.name == node) {
            continue;
        }
        out.push(GpuInfo {
            vendor: gpu_vendor(&node),
            name: node,
            // IOKit 不直接给显存总量；不编造
            memory_total: None,
            memory_used: pick(&values, "In use system memory").map(|v| v as u64),
            usage: pick(&values, "Device Utilization %").map(|v| v as f32),
            temperature_c: pick(&values, "Temperature(C)").map(|v| v as f32),
            power_w: None,
            driver_version: None,
        });
    }
    Ok(out)
}

/// `gpu.list`：GPU 列表。
///
/// # 返回值
/// [`GpuInfo`] 列表的 JSON 信封
fn op_gpu_list() -> String {
    match gpus() {
        Ok(v) if !v.is_empty() => ok_json(v),
        Ok(_) => {
            err_json("无法从 IOKit 读取 GPU 统计：ioreg 未返回 IOAccelerator 的性能统计字典")
        }
        Err(e) => err_json(e),
    }
}

/// `sensor.list`：硬件传感器。
///
/// # 返回值
/// 一律为失败信封（macOS 需访问 SMC）
fn op_sensor_list() -> String {
    unsupported_reason(
        "sensor.list",
        "硬件传感器需通过 AppleSMC（IOKit 私有接口，结构未公开）读取，无公开 API 且通常需 root；\
         不编造温度/风扇数据",
    )
}

/// 解析容量文本（如 `8 GB` / `16GB`）。
///
/// # 参数
/// * `text` - 容量文本
///
/// # 返回值
/// 字节数；单位无法识别时返回 `None`
fn parse_size_bytes(text: &str) -> Option<u64> {
    let tokens: Vec<&str> = text.trim().split_whitespace().collect();
    let token = tokens.first()?;
    let digits: String = token
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() {
        return None;
    }
    let rest: String = token.chars().skip(digits.chars().count()).collect();
    let unit = if rest.is_empty() {
        tokens.get(1).map(|s| s.to_ascii_uppercase()).unwrap_or_default()
    } else {
        rest.to_ascii_uppercase()
    };
    let multiplier = if unit.starts_with("TB") {
        1024.0 * 1024.0 * 1024.0 * 1024.0
    } else if unit.starts_with("GB") {
        1024.0 * 1024.0 * 1024.0
    } else if unit.starts_with("MB") {
        1024.0 * 1024.0
    } else if unit.starts_with("KB") {
        1024.0
    } else if unit.starts_with('B') {
        1.0
    } else {
        0.0
    };
    if multiplier <= 0.0 {
        return None;
    }
    let number = digits.parse::<f64>().ok()?;
    Some((number * multiplier) as u64)
}

/// 解析频率文本（如 `2667 MHz`）。
///
/// # 参数
/// * `text` - 频率文本
///
/// # 返回值
/// MHz 数值
fn parse_mhz(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

/// 在文本里按分隔符切一次。
///
/// # 参数
/// * `text` - 文本
/// * `sep` - 分隔字符
///
/// # 返回值
/// `(左, 右)`；无分隔符时返回 `None`
fn split_once_char(text: &str, sep: char) -> Option<(&str, &str)> {
    text.find(sep)
        .map(|i| (&text[..i], &text[i + sep.len_utf8()..]))
}

/// 判断某行是否为 `system_profiler` 的分节标题而非内存条插槽标题。
///
/// # 参数
/// * `name` - 去掉冒号后的标题文本
///
/// # 返回值
/// true 表示分节标题（应跳过）
fn is_memory_section_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "memory" | "memory slots"
    )
}

/// 结束当前内存条块的解析并落盘。
///
/// # 参数
/// * `out` - 结果列表
/// * `header` - 当前插槽名，落盘后置空
/// * `fields` - 当前块的键值对，落盘后清空
fn flush_memory_slot(
    out: &mut Vec<MemoryModule>,
    header: &mut Option<String>,
    fields: &mut Vec<(String, String)>,
) {
    if let Some(slot) = header.take() {
        if !fields.is_empty() {
            let get = |key: &str| -> Option<String> {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            };
            if let Some(size_text) = get("size") {
                if let Some(capacity) = parse_size_bytes(&size_text) {
                    out.push(MemoryModule {
                        slot,
                        capacity,
                        memory_type: get("type"),
                        speed_mhz: get("speed").as_deref().and_then(parse_mhz),
                        manufacturer: get("manufacturer"),
                        serial: get("serial number").or_else(|| get("serial")),
                    });
                }
            }
        }
    }
    fields.clear();
}

/// 解析 `system_profiler SPMemoryDataType` 的纯文本输出。
///
/// # 参数
/// * `text` - 命令输出
///
/// # 返回值
/// 内存条列表；Apple Silicon 统一内存没有独立内存条时返回空列表（属正常结果）
fn parse_memory_slots(text: &str) -> Vec<MemoryModule> {
    let mut out: Vec<MemoryModule> = Vec::new();
    let mut header: Option<String> = None;
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.contains(": ") {
            let name = trimmed.trim_end_matches(':').trim();
            if is_memory_section_header(name) {
                continue;
            }
            flush_memory_slot(&mut out, &mut header, &mut fields);
            header = Some(name.to_string());
            continue;
        }
        if indent >= 6 {
            if let Some((key, value)) = split_once_char(trimmed, ':') {
                fields.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
        }
    }
    flush_memory_slot(&mut out, &mut header, &mut fields);
    out
}

/// `memory.modules`：物理内存条。
///
/// # 返回值
/// [`MemoryModule`] 列表的 JSON 信封
fn op_memory_modules() -> String {
    match run_command("/usr/sbin/system_profiler", &["SPMemoryDataType"]) {
        Ok(text) => ok_json(parse_memory_slots(&text)),
        Err(e) => err_json(format!(
            "{}（memory.modules 依赖 system_profiler SPMemoryDataType）",
            e
        )),
    }
}

/// 解析 `kmutil showloaded` / `kextstat` 的单行。
///
/// 列含义：`Index Refs Address Size Wired Name (Version) <Linked Against>`。
///
/// # 参数
/// * `line` - 一行输出
///
/// # 返回值
/// 内核模块信息；非数据行返回 `None`
fn parse_kext_line(line: &str) -> Option<KernelModuleInfo> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }
    if parts[0].parse::<u32>().is_err() {
        return None;
    }
    let address = parts[2]
        .strip_prefix("0x")
        .and_then(|s| u64::from_str_radix(s, 16).ok());
    let size = parts[3]
        .strip_prefix("0x")
        .and_then(|s| u64::from_str_radix(s, 16).ok());
    let name = parts[5].to_string();
    if name.is_empty() {
        return None;
    }
    Some(KernelModuleInfo {
        name,
        path: None,
        base_address: address.map(|v| format!("0x{:x}", v)),
        size,
    })
}

/// 从 `kmutil showloaded` 或 `kextstat` 输出里抽取模块列表。
///
/// # 参数
/// * `text` - 命令输出
///
/// # 返回值
/// 内核模块列表
fn parse_kernel_modules(text: &str) -> Vec<KernelModuleInfo> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(m) = parse_kext_line(line) {
            out.push(m);
        }
    }
    out
}

/// `kernel.modules`：内核扩展列表。
///
/// macOS 11+ 已移除/限制 `kextstat`，优先用 `kmutil showloaded`。
///
/// # 返回值
/// [`KernelModuleInfo`] 列表的 JSON 信封
fn op_kernel_modules() -> String {
    if let Ok(text) = run_command("/usr/bin/kmutil", &["showloaded"]) {
        let modules = parse_kernel_modules(&text);
        if !modules.is_empty() {
            return ok_json(modules);
        }
    }
    if let Ok(text) = run_command("/usr/sbin/kextstat", &[]) {
        let modules = parse_kernel_modules(&text);
        if !modules.is_empty() {
            return ok_json(modules);
        }
    }
    err_json(
        "无法列出内核扩展：kmutil showloaded 与 kextstat 均无可用输出\
         （macOS 11+ 已限制 kextstat，部分系统需 root）",
    )
}

/// 解析 `launchctl list` 输出。
///
/// 格式：`PID\tStatus\tLabel`，其中 PID 为 `-` 表示未运行。
///
/// # 参数
/// * `text` - 命令输出
///
/// # 返回值
/// 服务列表
fn parse_launchctl(text: &str) -> Vec<ServiceInfo> {
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        if parts[0] == "PID" {
            continue;
        }
        let pid = parts[0].parse::<i32>().ok();
        let label = parts[2..].join(" ");
        if label.is_empty() {
            continue;
        }
        out.push(ServiceInfo {
            name: label.clone(),
            display_name: label,
            state: if pid.is_some() {
                "running".to_string()
            } else {
                "stopped".to_string()
            },
            // launchd 未在列表输出里给出启动类型/可执行路径，不猜测
            start_type: "unknown".to_string(),
            account: None,
            binary_path: None,
            is_driver: Some(false),
            pid,
        });
    }
    out
}

/// `service.list`：launchd 服务列表。
///
/// # 返回值
/// [`ServiceInfo`] 列表的 JSON 信封
fn op_service_list() -> String {
    match run_command("/bin/launchctl", &["list"]) {
        Ok(text) => ok_json(parse_launchctl(&text)),
        Err(e) => err_json(format!("{}（service.list 依赖 launchctl list）", e)),
    }
}

/// 读取套接字/网络连接（`lsof -n -P -i`）。
///
/// `lsof` 在"无匹配"时返回 1，因此这里不检查退出码，输出为空按正常空结果处理；
/// 命令无法启动（未找到 lsof）才算失败。
///
/// # 参数
/// * `pid` - 可选进程 ID，给定时只列该进程
///
/// # 返回值
/// 套接字列表；lsof 无法启动时返回错误描述
fn sockets(pid: Option<i32>) -> Result<Vec<SocketInfo>, String> {
    let mut owned: Vec<String> = vec!["-n".to_string(), "-P".to_string(), "-i".to_string()];
    if let Some(p) = pid {
        owned.push("-p".to_string());
        owned.push(p.to_string());
    }
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    let output = run_command_raw("/usr/sbin/lsof", &refs)?;
    Ok(parse_lsof(&String::from_utf8_lossy(&output.stdout)))
}

/// 从 `lsof -n -P -i` 的 NAME 列里抽出状态（括号内内容）。
///
/// # 参数
/// * `name` - NAME 列文本
///
/// # 返回值
/// 状态串；无括号时返回 `UNKNOWN`
fn lsof_state(name: &str) -> String {
    match (name.rfind('('), name.rfind(')')) {
        (Some(open), Some(close)) if close > open => name[open + 1..close].to_string(),
        _ => "UNKNOWN".to_string(),
    }
}

/// 解析 `lsof -n -P -i` 输出。
///
/// 列：`COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME`。
///
/// # 参数
/// * `text` - 命令输出
///
/// # 返回值
/// 套接字列表
fn parse_lsof(text: &str) -> Vec<SocketInfo> {
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        if parts[0] == "COMMAND" {
            continue;
        }
        let base = match parts[7].to_ascii_uppercase().as_str() {
            "TCP" => "tcp",
            "UDP" => "udp",
            _ => continue,
        };
        let protocol = if parts[4].eq_ignore_ascii_case("IPv6") {
            format!("{}6", base)
        } else {
            base.to_string()
        };
        let name = parts[8..].join(" ");
        let (local, remote) = match name.split_once("->") {
            Some((l, r)) => (l.trim().to_string(), Some(r.trim().to_string())),
            None => (name.trim().to_string(), None),
        };
        if local.is_empty() {
            continue;
        }
        out.push(SocketInfo {
            protocol,
            local,
            remote,
            state: lsof_state(&name),
            pid: parts[1].parse::<i32>().ok(),
            // lsof 的 DEVICE 列不是 inode，留空而不是填错
            inode: None,
        });
    }
    out
}

/// `socket.list`：套接字/网络连接。
///
/// # 参数
/// * `args` - 可选 `pid`
///
/// # 返回值
/// [`SocketInfo`] 列表的 JSON 信封
fn op_socket_list(args: &Value) -> String {
    let pid = arg_i64(args, &["pid"]).and_then(|v| i32::try_from(v).ok());
    match sockets(pid) {
        Ok(v) => ok_json(v),
        Err(e) => err_json(e),
    }
}

// ============================================================================
// 5. 动作
// ============================================================================

/// 向进程发送信号。
///
/// # 参数
/// * `pid` - 目标进程
/// * `sig` - 信号编号
///
/// # 返回值
/// 成功返回 `Ok(())`，失败返回错误描述
fn send_signal(pid: i32, sig: i32) -> Result<(), String> {
    let rc = unsafe { libc::kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(signal_err(pid))
    }
}

/// 信号发送失败的可读原因。
///
/// # 参数
/// * `pid` - 目标进程
///
/// # 返回值
/// 失败描述
fn signal_err(pid: i32) -> String {
    match Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => {
            format!("无权限向进程 {} 发送信号（需与目标同 uid 或 root）", pid)
        }
        Some(libc::ESRCH) => format!("进程 {} 不存在", pid),
        _ => format!("向进程 {} 发送信号失败: {}", pid, last_errno()),
    }
}

/// 设置进程 nice 值（优先级）。
///
/// # 参数
/// * `pid` - 目标进程
/// * `nice` - nice 值，越小优先级越高
///
/// # 返回值
/// 成功返回 `Ok(())`，失败返回错误描述
fn set_nice(pid: i32, nice: i32) -> Result<(), String> {
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
    if rc == 0 {
        return Ok(());
    }
    Err(match Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => format!(
            "无权限设置进程 {} 的优先级（提高优先级需 root，降低优先级才允许同 uid）",
            pid
        ),
        Some(libc::ESRCH) => format!("进程 {} 不存在", pid),
        _ => format!("设置进程 {} 的优先级失败: {}", pid, last_errno()),
    })
}

/// 把动作名归一化成 [`ActionKind`]。
///
/// 同时接受驼峰枚举名与下划线小写名，便于 Java 侧自由传参。
///
/// # 参数
/// * `raw` - 原始动作名
///
/// # 返回值
/// 匹配到的动作类型；无法识别返回 `None`
fn parse_action_kind(raw: &str) -> Option<ActionKind> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "terminate" | "terminateprocess" | "terminate_process" | "kill" | "killprocess" => {
            Some(ActionKind::TerminateProcess)
        }
        "suspend" | "suspendprocess" | "suspend_process" => Some(ActionKind::SuspendProcess),
        "resume" | "resumeprocess" | "resume_process" => Some(ActionKind::ResumeProcess),
        "setpriority" | "set_priority" | "priority" | "setnice" => Some(ActionKind::SetPriority),
        "suspendthread" | "suspend_thread" => Some(ActionKind::SuspendThread),
        "resumethread" | "resume_thread" => Some(ActionKind::ResumeThread),
        "setaffinity" | "set_affinity" => Some(ActionKind::SetAffinity),
        "closehandle" | "close_handle" => Some(ActionKind::CloseHandle),
        _ => None,
    }
}

/// `action.exec`：终止/挂起/恢复/设置优先级。
///
/// 注入类与线程级动作不在本模块能力范围内，一律显式不支持。
///
/// # 参数
/// * `args` - 含 `kind` / `target`（进程 ID）/ 可选 `arg`（set_priority 的 nice 值）
///
/// # 返回值
/// [`ActionResult`] 的 JSON 信封；动作不支持时是 `ok=false` 信封
fn op_action_exec(args: &Value) -> String {
    let raw_kind = match arg_str(args, &["kind", "action"]) {
        Some(v) => v,
        None => {
            return err_json("action.exec 缺少参数 kind");
        }
    };
    let target = match arg_str(args, &["target", "pid"]) {
        Some(v) => v,
        None => match arg_i64(args, &["target", "pid"]) {
            Some(v) => v.to_string(),
            None => {
                return err_json("action.exec 缺少参数 target（进程 ID）");
            }
        },
    };
    let pid = match target.trim().parse::<i32>() {
        Ok(v) => v,
        Err(_) => {
            return err_json(format!("action.exec 的 target 不是合法进程 ID: {}", target));
        }
    };
    let kind = match parse_action_kind(&raw_kind) {
        Some(k) => k,
        None => {
            return err_json(format!("无法识别的动作类型: {}", raw_kind));
        }
    };

    let result: Result<(), String> = match kind {
        ActionKind::TerminateProcess => send_signal(pid, libc::SIGKILL),
        ActionKind::SuspendProcess => send_signal(pid, libc::SIGSTOP),
        ActionKind::ResumeProcess => send_signal(pid, libc::SIGCONT),
        ActionKind::SetPriority => {
            let nice = arg_i64(args, &["arg", "nice", "priority"]).unwrap_or(0) as i32;
            set_nice(pid, nice)
        }
        ActionKind::SuspendThread | ActionKind::ResumeThread => {
            return unsupported_reason(
                "action.exec(线程挂起/恢复)",
                "macOS 无按线程挂起/恢复的用户态接口，需 task_for_pid + thread_suspend，\
                 受 SIP 与代码签名限制",
            );
        }
        ActionKind::SetAffinity => {
            return unsupported_reason(
                "action.exec(设置 CPU 亲和性)",
                "macOS 不提供进程级 CPU 亲和性设置（无 sched_setaffinity 等价接口）",
            );
        }
        ActionKind::CloseHandle => {
            return unsupported_reason(
                "action.exec(关闭句柄)",
                "跨进程关闭 fd 需注入或 ptrace 级能力，本模块不做注入类动作",
            );
        }
    };

    let action = ActionResult {
        kind,
        target,
        ok: result.is_ok(),
        error: result.err(),
    };
    ok_json(action)
}

// ============================================================================
// 6. 事件
// ============================================================================

/// `events.*`：事件驱动。
///
/// macOS 上没有授权就不可能拿到真实的事件流；这里明确不支持，
/// **不用轮询伪装成事件**。
///
/// # 参数
/// * `op` - 具体的 events 操作名
///
/// # 返回值
/// 一律为失败信封
fn op_events(op: &str) -> String {
    unsupported_reason(
        op,
        "事件驱动需 EndpointSecurity 框架及其 Apple 授权 entitlement\
         （com.apple.developer.endpoint-security.client），本模块未持有该签名授权；\
         不以轮询伪装成事件",
    )
}

// ============================================================================
// 7. 平台入口
// ============================================================================

// libSystem 里的 mach 接口。`mach_vm_region` 用于枚举本进程内存区域，
// `mach_port_deallocate` 回收 `mach_vm_region` 返回的对象端口。
//
// 这两个符号 `libc` 未导出（`libc` 只导出 `mach_vm_map` 等被标记 deprecated 的
// 子集），因此在这里自行声明；`cargo check` 不链接，真实链接由 libSystem 提供。
#[link(name = "System")]
extern "C" {
    /// 枚举目标任务的虚拟内存区域。
    ///
    /// # 参数
    /// * `target_task` - 目标任务端口
    /// * `address` - 入参为起始地址，出参为下一区域起始地址
    /// * `size` - 出参：区域大小
    /// * `flavor` - 信息结构类型，取 `VM_REGION_BASIC_INFO_64`
    /// * `info` - 出参：`natural_t` 数组
    /// * `info_cnt` - 入参为数组容量，出参为实际写入个数
    /// * `object_name` - 出参：区域对象端口，需由调用方释放
    ///
    /// # 返回值
    /// `kern_return_t`
    fn mach_vm_region(
        target_task: u32,
        address: *mut u64,
        size: *mut u64,
        flavor: i32,
        info: *mut i32,
        info_cnt: *mut u32,
        object_name: *mut u32,
    ) -> i32;

    /// 释放 mach 端口名。
    ///
    /// # 参数
    /// * `task` - 目标端口所在的任务
    /// * `name` - 端口名
    ///
    /// # 返回值
    /// `kern_return_t`
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
}

/// 平台入口：按 op 分发。
///
/// # 参数
/// * `op` - 操作名
/// * `args` - JSON 参数，无参时为空串
///
/// # 返回值
/// 序列化后的 `Envelope`
pub fn call(op: &str, args: &str) -> String {
    let value = parse_args(args);
    match op {
        // ---------- 进程 ----------
        "process.list" => op_process_list(),
        "process.tree" => ok_json(common::process_tree()),
        "process.detail" => op_process_detail(&value),
        "process.handles" => op_process_handles(&value),
        "process.credential" => op_process_credential(&value),
        "process.modules" => op_process_modules(&value),
        "process.threads" => op_process_threads(&value),
        "process.env" => op_process_env(&value),
        "process.mappings" => op_process_mappings(&value),
        "process.stack" => op_process_stack(&value),

        // ---------- 系统 ----------
        "disk.io" => op_disk_io(),
        "gpu.list" => op_gpu_list(),
        "sensor.list" => op_sensor_list(),
        "memory.modules" => op_memory_modules(),
        "kernel.modules" => op_kernel_modules(),
        "service.list" => op_service_list(),
        "socket.list" => op_socket_list(&value),

        // ---------- 动作与事件 ----------
        "action.exec" => op_action_exec(&value),
        "events.start" | "events.poll" | "events.stop" => op_events(op),

        other => err_json(unsupported(other, PLATFORM)),
    }
}
