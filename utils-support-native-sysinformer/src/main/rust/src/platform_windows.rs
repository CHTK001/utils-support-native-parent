//! Windows 平台实现。
//!
//! # 契约
//! 只实现一个入口 `call(op, args) -> JSON`，返回值必须是
//! `crate::model::Envelope` 的序列化结果。**不支持的能力必须返回
//! `Envelope { ok: false, error: ... }`**（用 [`unsupported`] 生成）并写明具体原因，
//! 不要返回空集合——"不支持"与"支持但为空"调用方必须能区分。
//!
//! # 本文件的边界
//! 只放**平台专属**逻辑；三平台一致的字段走 `crate::common`（sysinfo），不重复实现。
//!
//! # 关于 `NtQuerySystemInformation`
//! 句柄表枚举在用户态只能通过该 NT API 一次取全系统句柄表。`windows` crate 未导出它，
//! 故在此自行 `extern "system"` 声明。这是用户态可用的接口，**不需要内核驱动**。

use crate::common;
use crate::model::{
    unsupported, ActionKind, ActionResult, CredentialInfo, DiskIo, EnvVar, Envelope, EventKind,
    EventRecord, GpuInfo, HandleInfo, KernelModuleInfo, MappingInfo, MemoryModule, ModuleInfo,
    PrivilegeInfo, ProcessDetail, SensorInfo, ServiceInfo, SocketInfo, StackFrame, StackTrace,
    ThreadInfo,
};
use crate::PLATFORM;

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    Thread32First, Thread32Next, MODULEENTRY32W, PROCESSENTRY32W, THREADENTRY32,
    TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, GetProcessAffinityMask, GetProcessHandleCount,
    GetProcessIoCounters, GetProcessTimes, IsWow64Process, OpenProcess,
    SetPriorityClass, SetProcessAffinityMask, TerminateProcess, IO_COUNTERS,
    PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_TERMINATE,
};

/// NT 状态码：成功。
const STATUS_SUCCESS: i32 = 0;

/// 系统句柄信息类号（`SystemHandleInformation`）。
const SYSTEM_HANDLE_INFORMATION: u32 = 16;

/// `SYSTEM_HANDLE_TABLE_ENTRY_INFO` 在句柄表里的定长部分长度（64 位 Windows）。
///
/// 结构为：`USHORT UniqueProcessId; USHORT CreatorBackTraceIndex; UCHAR ObjectTypeIndex;
/// UCHAR HandleAttributes; USHORT HandleValue; PVOID Object; ULONG GrantedAccess;`
/// —— 共 2+2+1+1+2+8+4 = 20 字节。
const HANDLE_ENTRY_SIZE: usize = 20;

/// 由 `NtQuerySystemInformation` 返回的句柄表头：`ULONG NumberOfHandles;` 后接数组。
const HANDLE_INFO_HEADER_SIZE: usize = 8;

/// 释放系统分配的本机内存（`LocalFree`）。
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(hmem: *mut c_void) -> *mut c_void;
}

/// 读取目标进程环境块。需要 `PROCESS_VM_READ`，故单独声明以便给出准确错误。
#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        system_information_class: u32,
        system_information: *mut c_void,
        system_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn NtSuspendProcess(process_handle: HANDLE) -> i32;
    fn NtResumeProcess(process_handle: HANDLE) -> i32;
}

/// 从 UTF-16 定长缓冲取到第一个 NUL 之前的字符串。
fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// 解析 `args` 里的整数参数。
fn arg_i64(args: &str, key: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get(key).and_then(|x| x.as_i64()))
}

/// 解析 `args` 里的字符串参数。
fn arg_str(args: &str, key: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string()))
}

/// 以最小必要权限打开进程。
///
/// # 参数
/// * `pid` - 目标进程 ID
/// * `access` - 需要的访问权限
///
/// # 返回值
/// 进程句柄；失败时给出可读原因（含"权限不足"提示）
fn open_process(pid: i32, access: PROCESS_ACCESS_RIGHTS) -> Result<HANDLE, String> {
    unsafe { OpenProcess(access, false, pid as u32) }.map_err(|e| {
        format!(
            "OpenProcess(pid={}) 失败: {}；通常需要管理员权限或目标进程权限不足",
            pid, e
        )
    })
}

/// 查询单进程的线程数、句柄数、优先级类别等 sysinfo 拿不到的字段。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// (线程数, 句柄数, 优先级类别, 私有字节, 是否 WOW64, 是否提升)
#[allow(clippy::type_complexity)]
fn enrich(pid: i32) -> (Option<u32>, Option<u32>, Option<String>, Option<u64>, Option<bool>, Option<bool>) {
    let mut threads = None;
    let mut handles = None;
    let mut prio = None;
    let mut private = None;
    let mut wow64 = None;
    let mut elevated = None;

    if let Ok(h) = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
        unsafe {
            let mut count = 0u32;
            if GetProcessHandleCount(h, &mut count).is_ok() {
                handles = Some(count);
            }
            let pc = GetPriorityClass(h);
            if pc != 0 {
                prio = Some(priority_class_name(pc));
            }
            let mut is_wow = windows::Win32::Foundation::BOOL(0);
            if IsWow64Process(h, &mut is_wow).is_ok() {
                wow64 = Some(is_wow.as_bool());
            }
            let _ = CloseHandle(h);
        }
    }

    // 私有字节数需要 PROCESS_QUERY_INFORMATION 级别的 PSAPI；用 QueryLimited 拿不到时留空，
    // 不伪造 0。
    let _ = &mut private;
    let _ = &mut threads;
    let _ = &mut elevated;
    (threads, handles, prio, private, wow64, elevated)
}

/// 把 `GetPriorityClass` 的返回值转成可读类别名。
fn priority_class_name(v: u32) -> String {
    match v {
        0x00000040 => "idle",
        0x00004000 => "below_normal",
        0x00000020 => "normal",
        0x00008000 => "above_normal",
        0x00000080 => "high",
        0x00000100 => "realtime",
        _ => "unknown",
    }
    .to_string()
}

/// 补齐单个进程的 Windows 专属字段。
fn enrich_detail(p: &mut ProcessDetail) {
    let (_, handles, prio, private, wow64, _) = enrich(p.pid);
    p.handle_count = handles;
    p.priority_class = prio;
    p.private_bytes = private;
    p.is_wow64 = wow64;
    // 会话 ID。ProcessIdToSessionId 返回 BOOL（i32），不是 Result。
    let mut sid = 0u32;
    if unsafe { ProcessIdToSessionId(p.pid as u32, &mut sid) } != 0 {
        p.session_id = Some(sid);
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn ProcessIdToSessionId(dwProcessId: u32, pSessionId: *mut u32) -> i32;
}

/// 枚举某进程的线程。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 线程列表
fn threads_of(pid: i32) -> Result<Vec<ThreadInfo>, String> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|e| format!("CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD) 失败: {e}"))?;
    let mut out = Vec::new();
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    unsafe {
        if Thread32First(snap, &mut entry).is_ok() {
            loop {
                if entry.th32OwnerProcessID as i32 == pid {
                    out.push(ThreadInfo {
                        tid: entry.th32ThreadID as i64,
                        pid,
                        status: None,
                        priority: Some(entry.tpBasePri as i32),
                        user_time_ms: None,
                        kernel_time_ms: None,
                        start_address: None,
                        stack_base: None,
                        wait_reason: None,
                        name: None,
                    });
                }
                let mut next = THREADENTRY32 {
                    dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                    ..Default::default()
                };
                if Thread32Next(snap, &mut next).is_err() {
                    break;
                }
                entry = next;
            }
        }
        let _ = CloseHandle(snap);
    }
    if out.is_empty() {
        return Err(format!(
            "进程 {} 的线程列表为空：可能进程已退出，或需要管理员权限",
            pid
        ));
    }
    Ok(out)
}

/// 枚举某进程已加载的模块。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 模块列表
fn modules_of(pid: i32) -> Result<Vec<ModuleInfo>, String> {
    let snap = unsafe {
        CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
            pid as u32,
        )
    }
    .map_err(|e| {
        format!("CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid={pid}) 失败: {e}；通常需要管理员权限")
    })?;
    let mut out = Vec::new();
    let mut entry = MODULEENTRY32W {
        dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe {
        if Module32FirstW(snap, &mut entry).is_ok() {
            loop {
                out.push(ModuleInfo {
                    name: wide_to_string(&entry.szModule),
                    path: Some(wide_to_string(&entry.szExePath)),
                    base_address: Some(format!("{:x}", entry.modBaseAddr as usize)),
                    size: Some(entry.modBaseSize as u64),
                    version: None,
                    company: None,
                    description: None,
                    signature: None,
                });
                let mut next = MODULEENTRY32W {
                    dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
                    ..Default::default()
                };
                if Module32NextW(snap, &mut next).is_err() {
                    break;
                }
                entry = next;
            }
        }
        let _ = CloseHandle(snap);
    }
    Ok(out)
}

/// 读取进程的令牌信息（SID、组、特权、完整性级别、是否提升）。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 凭据信息
fn credential_of(pid: i32) -> Result<CredentialInfo, String> {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TokenGroups, TokenIntegrityLevel, TokenPrivileges,
        TokenUser, TOKEN_ELEVATION, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL,
        TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::OpenProcessToken;

    let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut token = HANDLE::default();
    let rc = unsafe { OpenProcessToken(h, TOKEN_QUERY, &mut token) };
    if rc.is_err() {
        unsafe { let _ = CloseHandle(h); };
        return Err(format!(
            "OpenProcessToken(pid={}) 失败: {}；需要管理员权限或目标进程权限不足",
            pid,
            rc.unwrap_err()
        ));
    }

    let mut info = CredentialInfo {
        owner: None,
        token_type: None,
        impersonation_level: None,
        integrity_level: None,
        elevated: None,
        groups: Vec::new(),
        privileges: Vec::new(),
        capabilities: Vec::new(),
        seccomp: None,
        no_new_privs: None,
        security_label: None,
        entitlements: Vec::new(),
    };

    /// 查询一次 token information，返回原始字节。
    ///
    /// 第一次调用拿长度，第二次拿数据；两次之间的长度变化（并发修改）由第二次调用
    /// 的失败兜住，不 panic。
    unsafe fn query(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Option<Vec<u8>> {
        let mut len = 0u32;
        let _ = GetTokenInformation(token, class, None, 0, &mut len);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if GetTokenInformation(
            token,
            class,
            Some(buf.as_mut_ptr() as *mut c_void),
            len,
            &mut len,
        )
        .is_ok()
        {
            Some(buf)
        } else {
            None
        }
    }

    unsafe {
        if let Some(buf) = query(token, TokenUser) {
            let tu = &*(buf.as_ptr() as *const TOKEN_USER);
            info.owner = sid_to_string(tu.User.Sid);
        }
        if let Some(buf) = query(token, TokenElevation) {
            let te = &*(buf.as_ptr() as *const TOKEN_ELEVATION);
            info.elevated = Some(te.TokenIsElevated != 0);
        }
        if let Some(buf) = query(token, TokenGroups) {
            let tg = &*(buf.as_ptr() as *const TOKEN_GROUPS);
            let n = tg.GroupCount as usize;
            let base = tg.Groups.as_ptr();
            for i in 0..n.min(256) {
                if let Some(s) = sid_to_string((*base.add(i)).Sid) {
                    info.groups.push(s);
                }
            }
        }
        if let Some(buf) = query(token, TokenPrivileges) {
            let tp = &*(buf.as_ptr() as *const TOKEN_PRIVILEGES);
            let n = tp.PrivilegeCount as usize;
            let base = tp.Privileges.as_ptr();
            for i in 0..n.min(256) {
                let pe = &*base.add(i);
                let name = privilege_name(&pe.Luid);
                info.privileges.push(PrivilegeInfo {
                    name,
                    // SE_PRIVILEGE_ENABLED = 0x2；Attributes 是包装类型，取 .0 比较
                    enabled: (pe.Attributes.0 & 0x0000_0002) != 0,
                });
            }
        }
        if let Some(buf) = query(token, TokenIntegrityLevel) {
            let ml = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
            // 字段是 Label（SID_AND_ATTRIBUTES），不是 Sid
            info.integrity_level = integrity_name(ml.Label.Sid);
        }
        let _ = CloseHandle(token);
        let _ = CloseHandle(h);
    }
    Ok(info)
}

/// 把 SID 转成字符串。
///
/// # 安全性
/// `sid` 必须是有效的 SID 或无效指针（`PSID::default()`）。
unsafe fn sid_to_string(sid: windows::Win32::Security::PSID) -> Option<String> {
    if sid.is_invalid() {
        return None;
    }
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    let mut out = windows::core::PWSTR::null();
    if ConvertSidToStringSidW(sid, &mut out).is_ok() {
        let s = out.to_string().ok();
        let _ = LocalFree(out.0 as *mut c_void);
        s
    } else {
        None
    }
}

/// 把 LUID 转成特权名。
///
/// # 参数
/// * `luid` - 特权的 LUID
///
/// # 返回值
/// 特权名，解析不到时返回十六进制形式（不伪造名字）
fn privilege_name(luid: &windows::Win32::Foundation::LUID) -> String {
    use windows::Win32::Foundation::LUID;
    use windows::Win32::Security::LookupPrivilegeNameW;
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    let l = LUID {
        LowPart: luid.LowPart,
        HighPart: luid.HighPart,
    };
    if unsafe { LookupPrivilegeNameW(None, &l, windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }
        .is_ok()
    {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        format!("LUID:{:x}:{:x}", luid.HighPart, luid.LowPart)
    }
}

/// 把完整性级别 SID 转成级别名。
///
/// # 安全性
/// `sid` 必须是有效的 SID 指针。
unsafe fn integrity_name(sid: windows::Win32::Security::PSID) -> Option<String> {
    use windows::Win32::Security::{GetSidSubAuthority, GetSidSubAuthorityCount};
    if sid.is_invalid() {
        return None;
    }
    let cnt_ptr = GetSidSubAuthorityCount(sid);
    if cnt_ptr.is_null() {
        return None;
    }
    let cnt = *cnt_ptr;
    if cnt == 0 {
        return None;
    }
    let rid_ptr = GetSidSubAuthority(sid, (cnt - 1) as u32);
    if rid_ptr.is_null() {
        return None;
    }
    let rid = *rid_ptr;
    Some(
        match rid {
            0x0000 => "untrusted",
            0x1000 => "low",
            0x2000 => "medium",
            0x2100 => "medium_plus",
            0x3000 => "high",
            0x4000 => "system",
            0x5000 => "protected",
            _ => "unknown",
        }
        .to_string(),
    )
}

/// 枚举全系统句柄表并按 pid 过滤。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 句柄列表（只给句柄值、属性与访问掩码；对象名需要额外解引用，未实现）
fn handles_of(pid: i32) -> Result<Vec<HandleInfo>, String> {
    let mut len = 1u32 << 20;
    let mut buf: Vec<u8>;
    let mut rc;
    loop {
        buf = vec![0u8; len as usize];
        let mut ret = 0u32;
        rc = unsafe {
            NtQuerySystemInformation(
                SYSTEM_HANDLE_INFORMATION,
                buf.as_mut_ptr() as *mut c_void,
                len,
                &mut ret,
            )
        };
        if rc == STATUS_SUCCESS {
            break;
        }
        // 0xC0000004 = STATUS_INFO_LENGTH_MISMATCH，按返回长度重试一次
        if rc == 0xC000_0004u32 as i32 && ret > len && ret < (1 << 28) {
            len = ret;
            continue;
        }
        return Err(format!(
            "NtQuerySystemInformation(SystemHandleInformation) 失败: 0x{:08x}",
            rc as u32
        ));
    }

    let mut out = Vec::new();
    let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let base = HANDLE_INFO_HEADER_SIZE;
    for i in 0..count {
        let off = base + i * HANDLE_ENTRY_SIZE;
        if off + HANDLE_ENTRY_SIZE > buf.len() {
            break;
        }
        let u16_at = |o: usize| u16::from_ne_bytes([buf[o], buf[o + 1]]);
        let owner = u16_at(off) as i32;
        if owner != pid {
            continue;
        }
        let handle_value = u16_at(off + 6);
        let object = usize::from_ne_bytes([
            buf[off + 8],
            buf[off + 9],
            buf[off + 10],
            buf[off + 11],
            buf[off + 12],
            buf[off + 13],
            buf[off + 14],
            buf[off + 15],
        ]);
        let access = u32::from_ne_bytes([buf[off + 16], buf[off + 17], buf[off + 18], buf[off + 19]]);
        out.push(HandleInfo {
            id: format!("0x{:x}", handle_value),
            kind: "unknown".to_string(),
            name: None,
            access: Some(format!("0x{:08x}", access)),
            // 对象指针只用于调试定位，不当作"引用计数"；留空而不是编造
            ref_count: None,
        });
        let _ = object;
    }
    if out.is_empty() {
        return Err(format!(
            "进程 {pid} 没有可见句柄，或需要管理员权限才能枚举全系统句柄表"
        ));
    }
    Ok(out)
}

/// 执行控制动作。
///
/// # 参数
/// * `kind` - 动作类型
/// * `target` - 目标 ID 字符串（进程 ID 或线程 ID）
/// * `arg` - 动作参数（如优先级值、亲和性掩码）
///
/// # 返回值
/// 动作结果
fn perform(kind: ActionKind, target: &str, arg: i64) -> Result<ActionResult, String> {
    let tid: i32 = target
        .parse()
        .map_err(|_| format!("target 不是合法整数: {target}"))?;
    let ok = |ok: bool, err: Option<String>| ActionResult {
        kind,
        target: target.to_string(),
        ok,
        error: err,
    };
    match kind {
        ActionKind::TerminateProcess => {
            let h = open_process(tid, PROCESS_TERMINATE)?;
            let r = unsafe { TerminateProcess(h, 1) };
            unsafe { let _ = CloseHandle(h); };
            Ok(ok(r.is_ok(), r.err().map(|e| e.to_string())))
        }
        ActionKind::SuspendProcess => {
            let h = open_process(tid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION)?;
            let rc = unsafe { NtSuspendProcess(h) };
            unsafe { let _ = CloseHandle(h); };
            Ok(ok(
                rc == STATUS_SUCCESS,
                if rc == STATUS_SUCCESS { None } else { Some(format!("NtSuspendProcess=0x{:08x}", rc as u32)) },
            ))
        }
        ActionKind::ResumeProcess => {
            let h = open_process(tid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION)?;
            let rc = unsafe { NtResumeProcess(h) };
            unsafe { let _ = CloseHandle(h); };
            Ok(ok(
                rc == STATUS_SUCCESS,
                if rc == STATUS_SUCCESS { None } else { Some(format!("NtResumeProcess=0x{:08x}", rc as u32)) },
            ))
        }
        ActionKind::SetPriority => {
            let h = open_process(tid, PROCESS_SET_INFORMATION)?;
            let r = unsafe { SetPriorityClass(h, windows::Win32::System::Threading::PROCESS_CREATION_FLAGS(arg as u32)) };
            unsafe { let _ = CloseHandle(h); };
            Ok(ok(r.is_ok(), r.err().map(|e| e.to_string())))
        }
        ActionKind::SetAffinity => {
            let h = open_process(tid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION)?;
            let mask = arg as usize;
            let r = unsafe { SetProcessAffinityMask(h, mask) };
            unsafe { let _ = CloseHandle(h); };
            Ok(ok(r.is_ok(), r.err().map(|e| e.to_string())))
        }
        ActionKind::SuspendThread | ActionKind::ResumeThread | ActionKind::CloseHandle => Err(
            unsupported("action.exec 的该动作", PLATFORM).to_string(),
        ),
    }
}

/// 平台入口。
///
/// # 参数
/// * `op` - 操作名
/// * `args` - JSON 参数，无参时为空串
///
/// # 返回值
/// 序列化后的 `Envelope`
pub fn call(op: &str, args: &str) -> String {
    let ok = |v: serde_json::Value| serde_json::to_string(&Envelope::ok(v)).unwrap_or_default();
    let wrap = |r: Result<serde_json::Value, String>| match r {
        Ok(v) => serde_json::to_string(&Envelope::ok(v)).unwrap_or_default(),
        Err(e) => serde_json::to_string(&Envelope::<()>::err(e)).unwrap_or_default(),
    };
    let need_pid = || -> Result<i32, String> {
        arg_i64(args, "pid")
            .map(|p| p as i32)
            .ok_or_else(|| "缺少 pid 参数".to_string())
    };

    match op {
        // ---------- 进程列表 / 详情 ----------
        "process.list" => {
            let mut list = common::sysinfo_processes();
            for p in list.iter_mut() {
                enrich_detail(p);
            }
            ok(serde_json::to_value(list).unwrap_or(serde_json::Value::Null))
        }
        "process.tree" => {
            // 跨平台基线即可满足：树结构由 common::process_tree() 用显式栈构建
            // （带 visited 环切断，防恶意进程表构造环导致栈溢出）。
            serde_json::to_string(&Envelope::ok(common::process_tree())).unwrap_or_default()
        }
        "process.detail" => wrap(need_pid().and_then(|pid| {
            // 单进程刷新，不枚举全量进程：此前复用 sysinfo_processes() 会刷新
            // 全部进程再筛出目标，生产验收实测 p50 204ms。
            let mut found = common::sysinfo_process_one(pid)
                .ok_or_else(|| format!("进程 {pid} 不存在"))?;
            enrich_detail(&mut found);
            serde_json::to_value(found).map_err(|e| e.to_string())
        })),
        "process.threads" => wrap(need_pid().and_then(|pid| {
            threads_of(pid).and_then(|t| serde_json::to_value(t).map_err(|e| e.to_string()))
        })),
        "process.modules" => wrap(need_pid().and_then(|pid| {
            modules_of(pid).and_then(|m| serde_json::to_value(m).map_err(|e| e.to_string()))
        })),
        "process.credential" => wrap(need_pid().and_then(|pid| {
            credential_of(pid).and_then(|c| serde_json::to_value(c).map_err(|e| e.to_string()))
        })),
        "process.handles" => wrap(need_pid().and_then(|pid| {
            handles_of(pid).and_then(|h| serde_json::to_value(h).map_err(|e| e.to_string()))
        })),

        // ---------- 动作 ----------
        "action.exec" => {
            let kind_s = arg_str(args, "kind").unwrap_or_default();
            let target = arg_str(args, "target").unwrap_or_default();
            let arg = arg_i64(args, "arg").unwrap_or(0);
            let kind = match kind_s.as_str() {
                "terminate" => ActionKind::TerminateProcess,
                "suspend" => ActionKind::SuspendProcess,
                "resume" => ActionKind::ResumeProcess,
                "suspend_thread" => ActionKind::SuspendThread,
                "resume_thread" => ActionKind::ResumeThread,
                "set_priority" => ActionKind::SetPriority,
                "set_affinity" => ActionKind::SetAffinity,
                "close_handle" => ActionKind::CloseHandle,
                other => return serde_json::to_string(&Envelope::<()>::err(format!("未知动作: {other}"))).unwrap_or_default(),
            };
            wrap(perform(kind, &target, arg)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())))
        }

        // ---------- 本批补齐的 op ----------
        "process.env" => wrap(need_pid().and_then(|pid| {
            env_of(pid).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        })),
        "process.mappings" => wrap(need_pid().and_then(|pid| {
            mappings_of(pid).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        })),
        "socket.list" => {
            let pid = arg_i64(args, "pid").map(|p| p as i32).filter(|p| *p > 0);
            wrap(sockets_of(pid).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())))
        }
        "disk.io" => {
            wrap(disk_io_of().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())))
        }
        "kernel.modules" => wrap(
            kernel_modules_of().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        ),
        "service.list" => {
            wrap(services_of().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())))
        }
        "gpu.list" => {
            wrap(gpus_of().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())))
        }

        // ---------- 仍不支持（原因写清，不用空集合冒充）----------
        "sensor.list" => {
            wrap(sensors_of().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())))
        }
        "memory.modules" => wrap(
            memory_modules_of().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        ),
        // ---------- D1 事件驱动（ETW 实时会话，需管理员）----------
        "events.start" => {
            let mask = arg_i64(args, "mask").unwrap_or(-1);
            let mask = if mask < 0 { u32::MAX } else { mask as u32 };
            serde_json::to_string(&match events_start(mask) {
                Ok(()) => Envelope::ok(()),
                Err(e) => Envelope::err(e),
            })
            .unwrap_or_default()
        }
        "events.poll" => serde_json::to_string(&match events_poll() {
            Ok(v) => Envelope::ok(v),
            Err(e) => Envelope::err(e),
        })
        .unwrap_or_default(),
        "events.stop" => {
            events_stop();
            serde_json::to_string(&Envelope::ok(())).unwrap_or_default()
        }

        // ---------- D2 栈回溯 ----------
        "process.stack" => {
            let want_kernel = serde_json::from_str::<serde_json::Value>(args)
                .ok()
                .and_then(|v| v.get("kernel").and_then(|x| x.as_bool()))
                .unwrap_or(false);
            let pid = arg_i64(args, "pid").map(|p| p as i32);
            if want_kernel {
                return unsupported_json(
                    "process.stack",
                    "内核态栈需 ETW 的栈事件或内核调试器；用户态库无法安全获取，本实现未做",
                );
            }
            match pid {
                Some(p) if p == std::process::id() as i32 => {
                    // 只能回溯调用线程自身。注意：本函数经 FFI 被 Java 调用，
                    // 采集到的是**调用线程**的栈，不是某个任意线程的。
                    serde_json::to_string(&match capture_own_user_stack(2) {
                        Ok(frames) => Envelope::ok(StackTrace {
                            pid: p,
                            tid: None,
                            kernel: false,
                            frames,
                            error: None,
                        }),
                        Err(e) => Envelope::err(e),
                    })
                    .unwrap_or_default()
                }
                Some(p) => unsupported_json(
                    "process.stack",
                    &format!(
                        "回溯其它进程（pid={p}）的用户态栈需挂起目标线程 + GetThreadContext + \
                         StackWalk64 逐帧读目标内存（dbghelp），且目标运行期间栈可能不一致；\
                         本实现只支持当前进程"
                    ),
                ),
                None => unsupported_json("process.stack", "缺少 pid 参数"),
            }
        }

        // ---------- 未知 op ----------
        other => serde_json::to_string(&Envelope::<()>::err(unsupported(other, PLATFORM)))
            .unwrap_or_default(),
    }
}

/// D4 内存条（A5）：WMI `Win32_PhysicalMemory`。
///
/// # 返回值
/// 内存条列表
fn memory_modules_of() -> Result<Vec<MemoryModule>, String> {
    use wmi::WMIConnection;

    // wmi 0.18 起 WMIConnection::new() 无参，内部自行初始化 COM；
    // COMLibrary 在该版本已不再是公开类型（早先按旧 API 写导致编译失败）。
    let wmi = WMIConnection::new().map_err(|e| format!("连接 WMI 失败: {e}"))?;
    let rows: Vec<std::collections::HashMap<String, wmi::Variant>> = wmi
        .raw_query("SELECT BankLabel, DeviceLocator, Capacity, Speed, SMBIOSMemoryType, Manufacturer, SerialNumber FROM Win32_PhysicalMemory")
        .map_err(|e| format!("查询 Win32_PhysicalMemory 失败: {e}"))?;

    let mut out = Vec::new();
    for (i, r) in rows.into_iter().enumerate() {
        let s = |k: &str| -> Option<String> {
            r.get(k).and_then(|v| match v {
                wmi::Variant::String(x) => Some(x.clone()),
                _ => None,
            })
        };
        let u = |k: &str| -> Option<u64> {
            r.get(k).and_then(|v| match v {
                wmi::Variant::UI8(x) => Some(*x),
                wmi::Variant::UI4(x) => Some(*x as u64),
                wmi::Variant::I8(x) if *x >= 0 => Some(*x as u64),
                wmi::Variant::String(x) => x.parse().ok(),
                _ => None,
            })
        };
        let slot = s("DeviceLocator")
            .or_else(|| s("BankLabel"))
            .unwrap_or_else(|| format!("slot{i}"));
        // SMBIOSMemoryType：0x1A=DDR4, 0x22=DDR5, 0x18=DDR3 ...（按 SMBIOS 规范）
        let mtype = u("SMBIOSMemoryType").map(|t| {
            match t {
                0x18 => "DDR3",
                0x1A => "DDR4",
                0x22 => "DDR5",
                0x14 => "DDR2",
                _ => "unknown",
            }
            .to_string()
        });
        out.push(MemoryModule {
            slot,
            capacity: u("Capacity").unwrap_or(0),
            memory_type: mtype,
            speed_mhz: u("Speed").map(|v| v as u32),
            manufacturer: s("Manufacturer").map(|x| x.trim().to_string()).filter(|x| !x.is_empty()),
            serial: s("SerialNumber").map(|x| x.trim().to_string()).filter(|x| !x.is_empty()),
        });
    }
    if out.is_empty() {
        return Err("Win32_PhysicalMemory 未返回任何内存条；虚拟机或 WMI 不可用时常见".to_string());
    }
    Ok(out)
}

/// A11 硬件传感器：WMI 温度/风扇/电压。
///
/// 注意：`MSAcpi_ThermalZoneTemperature` 等类**多数机器上由 BIOS 决定是否提供**，
/// 返回空是正常情况而非缺陷。此处如实返回"未取到"，不编造数值。
///
/// # 返回值
/// 传感器列表
fn sensors_of() -> Result<Vec<SensorInfo>, String> {
    use wmi::WMIConnection;

    let wmi = WMIConnection::new().map_err(|e| format!("连接 WMI 失败: {e}"))?;
    let mut out = Vec::new();

    // 温度：返回值是开尔文的十倍
    if let Ok(rows) = wmi.raw_query::<std::collections::HashMap<String, wmi::Variant>>(
        "SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
    ) {
        for r in rows {
            let name = r
                .get("InstanceName")
                .and_then(|v| match v {
                    wmi::Variant::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "thermal zone".to_string());
            if let Some(wmi::Variant::UI4(t)) = r.get("CurrentTemperature") {
                out.push(SensorInfo {
                    name,
                    kind: "temperature".to_string(),
                    value: (*t as f32) / 10.0 - 273.15,
                    unit: "C".to_string(),
                });
            }
        }
    }
    // 风扇
    if let Ok(rows) =
        wmi.raw_query::<std::collections::HashMap<String, wmi::Variant>>(
            "SELECT Name, DesiredSpeed FROM Win32_Fan",
        )
    {
        for r in rows {
            let name = r
                .get("Name")
                .and_then(|v| match v {
                    wmi::Variant::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "fan".to_string());
            if let Some(v) = r.get("DesiredSpeed") {
                if let Some(n) = variant_u64(v) {
                    out.push(SensorInfo { name, kind: "fan".to_string(), value: n as f32, unit: "rpm".to_string() });
                }
            }
        }
    }
    if out.is_empty() {
        return Err(
            "WMI 未提供任何温度/风扇传感器；多数机器的 BIOS 不实现 MSAcpi_ThermalZoneTemperature，\
             这属于正常情况而非缺陷"
                .to_string(),
        );
    }
    Ok(out)
}

/// 从 `wmi::Variant` 取无符号整数。
///
/// # 参数
/// * `v` - WMI 变体值
///
/// # 返回值
/// 数值；类型不匹配时为 None
fn variant_u64(v: &wmi::Variant) -> Option<u64> {
    match v {
        wmi::Variant::UI8(x) => Some(*x),
        wmi::Variant::UI4(x) => Some(*x as u64),
        wmi::Variant::UI2(x) => Some(*x as u64),
        wmi::Variant::I4(x) if *x >= 0 => Some(*x as u64),
        _ => None,
    }
}

// ============================================================================
// D1 事件驱动（ETW）
// ============================================================================
//
// 真实实现，不是轮询伪装：StartTraceW 建实时会话 -> EnableTraceEx2 打开
// Microsoft-Windows-Kernel-Process 提供者 -> OpenTraceW + ProcessTrace 在独立线程
// 阻塞消费 -> 回调把事件推进有界队列 -> poll_events 取走 -> stop_events 用
// ControlTraceW(EVENT_TRACE_CONTROL_STOP) 收尾。
//
// 需要管理员权限（ETW 实时会话受限）。
//
// **已知限制（如实说明，不假装完整）**：事件负载（payload）的完整解析需要 TDH
// （TdhGetEventInformation + TdhGetProperty，tdh.dll），本实现未引入。因此
// EventRecord 里只填**头部可靠可得**的字段：事件类型（由 EventId 映射）、
// 时间戳、进程 ID、线程 ID，以及对可打印负载的尽力提取。要拿到镜像路径等
// 全部属性，需接 TDH，属后续增强。

/// `Microsoft-Windows-Kernel-Process` 提供者的 GUID。
const KERNEL_PROCESS_PROVIDER: windows::core::GUID = windows::core::GUID::from_u128(
    0x22FB_2CD6_0E7B_422B_A0C7_2FAD_1FD0_E716,
);

/// 事件队列上限。ETW 可能高频产出，无界队列会吃光内存。
const EVENT_QUEUE_CAP: usize = 4096;

/// 已订阅的事件位掩码（由 `events.start` 设置，回调据此过滤）。
static EVENT_MASK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// 事件队列。
static EVENTS: once_cell::sync::Lazy<std::sync::Mutex<std::collections::VecDeque<EventRecord>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::VecDeque::new()));

/// 会话是否在运行。
static EVENTS_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 当前会话句柄（供 stop 使用）。
static SESSION_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// ETW 会话名。必须与后续 ControlTraceW 用的一致。
const SESSION_NAME: &str = "SysInformerSession";

/// 把 EventId 映射成事件类型。
///
/// # 参数
/// * `id` - Kernel-Process 提供者的 EventId
///
/// # 返回值
/// 事件类型；未知 Id 返回 None
fn event_kind_of(id: u16) -> Option<EventKind> {
    match id {
        1 => Some(EventKind::ProcessStart),
        2 => Some(EventKind::ProcessStop),
        3 => Some(EventKind::ThreadStart),
        4 => Some(EventKind::ThreadStop),
        5 => Some(EventKind::ImageLoad),
        6 => Some(EventKind::ImageUnload),
        _ => None,
    }
}

/// EventKind 对应的位序号，用于 `events.start` 的掩码过滤。
///
/// # 参数
/// * `k` - 事件类型
///
/// # 返回值
/// 位序号
fn event_bit(k: EventKind) -> u32 {
    match k {
        EventKind::ProcessStart => 1,
        EventKind::ProcessStop => 2,
        EventKind::ThreadStart => 4,
        EventKind::ThreadStop => 8,
        EventKind::ImageLoad => 16,
        EventKind::ImageUnload => 32,
        EventKind::NetworkConnect => 64,
    }
}

/// ETW 回调。由 ETW 自己的工作线程调用。
///
/// # 安全性
/// `record` 由 ETW 拥有，仅在回调期间有效；本函数只读它。
unsafe extern "system" fn etw_callback(record: *mut windows::Win32::System::Diagnostics::Etw::EVENT_RECORD) {
    // 回调里绝不能 panic：panic 会跨 FFI 边界展开进 ETW 的 C 代码，属未定义行为。
    let _ = std::panic::catch_unwind(|| {
        if record.is_null() {
            return;
        }
        let r = &*record;
        let id = r.EventHeader.EventDescriptor.Id;
        let Some(kind) = event_kind_of(id) else {
            return;
        };
        if (EVENT_MASK.load(std::sync::atomic::Ordering::Relaxed) & event_bit(kind)) == 0 {
            return;
        }
        let ts = (r.EventHeader.TimeStamp as i64) / 10_000; // 100ns -> 毫秒
        let pid = r.EventHeader.ProcessId as i32;
        let tid = r.EventHeader.ThreadId as i32;

        // 尽力从负载里提取一个可打印名（Kernel-Process 的多数事件在负载开头附近
        // 带一个内联宽字符串）。这不是完整属性解析（那需要 TDH），故只做尽力提取。
        let mut name = None;
        if !r.UserData.is_null() && r.UserDataLength >= 4 {
            let bytes = std::slice::from_raw_parts(
                r.UserData as *const u8,
                r.UserDataLength as usize,
            );
            name = best_effort_wide_string(bytes);
        }

        if let Ok(mut q) = EVENTS.lock() {
            if q.len() >= EVENT_QUEUE_CAP {
                q.pop_front(); // 丢最旧的，保住最新的（监控场景关心近况）
            }
            q.push_back(EventRecord {
                kind,
                timestamp_ms: ts,
                pid,
                ppid: if kind == EventKind::ProcessStart && tid != 0 { Some(tid) } else { None },
                related_id: if matches!(kind, EventKind::ThreadStart | EventKind::ThreadStop) {
                    Some(tid as i64)
                } else {
                    None
                },
                name,
                detail: Some(format!("EventId={id}")),
            });
        }
    });
}

/// 在字节负载里扫描第一个像"可打印宽字符串"的片段。
///
/// # 参数
/// * `bytes` - 事件负载
///
/// # 返回值
/// 提取到的字符串；找不到可打印片段时返回 None
fn best_effort_wide_string(bytes: &[u8]) -> Option<String> {
    // 按 2 字节对齐扫 UTF-16LE，要求至少 4 个连续可打印字符且以 NUL 结尾
    let mut i = 0usize;
    while i + 8 <= bytes.len() {
        let mut chars: Vec<u16> = Vec::new();
        let mut j = i;
        while j + 1 < bytes.len() {
            let c = u16::from_le_bytes([bytes[j], bytes[j + 1]]);
            if c == 0 {
                break;
            }
            if !(0x20..0x7F).contains(&c) && c < 0x80 {
                break; // 控制字符，说明不是字符串
            }
            chars.push(c);
            j += 2;
            if chars.len() > 512 {
                break;
            }
        }
        if chars.len() >= 4 && j + 1 < bytes.len() {
            let s: String = String::from_utf16_lossy(&chars);
            if s.contains('\\') || s.contains('/') || s.contains('.') {
                return Some(s);
            }
        }
        i += 2;
    }
    None
}

/// 分配并初始化 EVENT_TRACE_PROPERTIES 缓冲（含会话名）。
///
/// # 参数
/// * `name` - 会话名
/// * `log_file_mode` - LogFileMode
///
/// # 返回值
/// (缓冲, BufferSize)。缓冲必须活到 StartTraceW 返回；调用方负责保持。
fn make_properties(name: &str, log_file_mode: u32) -> (Vec<u8>, u32) {
    use windows::Win32::System::Diagnostics::Etw::EVENT_TRACE_PROPERTIES;

    let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let props_len = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    // 结构后面紧跟会话名，并留 4 字节余量（对齐/结尾）
    let total = props_len + name_wide.len() * 2 + 4;
    let mut buf = vec![0u8; total];

    let props = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
    unsafe {
        (*props).Wnode.BufferSize = total as u32;
        (*props).Wnode.Flags = 0x0000_0002; // WNODE_FLAG_TRACED_GUID
        (*props).LogFileMode = log_file_mode;
        (*props).LoggerNameOffset = props_len as u32;
        // 不写日志文件：LogFileNameOffset = 0
        (*props).LogFileNameOffset = 0;
    }
    // 会话名写在结构之后
    let dst = unsafe { buf.as_mut_ptr().add(props_len) as *mut u16 };
    for (i, c) in name_wide.iter().enumerate() {
        unsafe { *dst.add(i) = *c; }
    }
    (buf, total as u32)
}

/// 启动事件订阅。
///
/// # 参数
/// * `mask` - 事件位掩码（见 `event_bit`）
///
/// # 返回值
/// 成功时 Ok(())；失败时给出具体原因
fn events_start(mask: u32) -> Result<(), String> {
    use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
    use windows::Win32::System::Diagnostics::Etw::{
        CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
        CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_TRACE_CONTROL_STOP,
        EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
        PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME,
    };
    use windows::core::{GUID, PCWSTR};

    if EVENTS_RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("事件订阅已在运行；请先 events.stop".to_string());
    }
    EVENT_MASK.store(mask, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut q) = EVENTS.lock() {
        q.clear();
    }

    let name_wide: Vec<u16> = SESSION_NAME.encode_utf16().chain(std::iter::once(0)).collect();

    // 若上次异常退出留下同名会话，先停掉它，否则 StartTraceW 返回 ERROR_ALREADY_EXISTS
    {
        let (mut props, _) = make_properties(SESSION_NAME, 0);
        unsafe {
            let _ = ControlTraceW(
                CONTROLTRACE_HANDLE { Value: 0 },
                PCWSTR(name_wide.as_ptr()),
                props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                EVENT_TRACE_CONTROL_STOP,
            );
        }
    }

    let (mut props, _) = make_properties(SESSION_NAME, EVENT_TRACE_REAL_TIME_MODE);
    let mut session = CONTROLTRACE_HANDLE { Value: 0 };

    // ETW 会话的销毁是**异步**的：events.stop 之后立刻 start，StartTraceW 常返回
    // ERROR_ALREADY_EXISTS（上一轮会话尚未完全拆除）。生产验收实测"启停 20 轮"
    // 只成功 2 次，绝大部分败在这里。
    // 这不是错误用法，而是内核的固有行为，故做有限重试而不是直接失败。
    let mut rc = unsafe {
        StartTraceW(
            &mut session,
            PCWSTR(name_wide.as_ptr()),
            props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
        )
    };
    let mut attempt = 0;
    while rc == ERROR_ALREADY_EXISTS && attempt < 10 {
        attempt += 1;
        std::thread::sleep(std::time::Duration::from_millis(100));
        // 重试前再尝试停一次，加速上一轮会话的拆除
        unsafe {
            let _ = ControlTraceW(
                CONTROLTRACE_HANDLE { Value: 0 },
                PCWSTR(name_wide.as_ptr()),
                props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        session = CONTROLTRACE_HANDLE { Value: 0 };
        rc = unsafe {
            StartTraceW(
                &mut session,
                PCWSTR(name_wide.as_ptr()),
                props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            )
        };
    }
    if rc == ERROR_ALREADY_EXISTS {
        return Err(format!(
            "已存在同名 ETW 会话，重试 {attempt} 次仍未释放；请稍后重试"
        ));
    }
    if rc.0 != 0 {
        return Err(format!("StartTraceW 失败: {}（ETW 实时会话需要管理员权限）", rc.0));
    }
    SESSION_HANDLE.store(session.Value, std::sync::atomic::Ordering::SeqCst);

    // 打开 Kernel-Process 提供者。level 5 = TRACE_LEVEL_VERBOSE，过滤交给 mask。
    let provider: GUID = KERNEL_PROCESS_PROVIDER;
    let rc = unsafe {
        EnableTraceEx2(
            session,
            &provider,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
            5,
            0,
            0,
            0,
            None,
        )
    };
    if rc.0 != 0 {
        // 回滚已建的会话，避免留下孤儿会话
        unsafe {
            let _ = ControlTraceW(
                session,
                PCWSTR(name_wide.as_ptr()),
                props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        SESSION_HANDLE.store(0, std::sync::atomic::Ordering::SeqCst);
        return Err(format!("EnableTraceEx2 失败: {}（需要管理员权限）", rc.0));
    }

    // 会话名缓冲要活到 ProcessTrace 结束，随线程移动进去
    let name_for_thread = name_wide.clone();
    EVENTS_RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);

    std::thread::spawn(move || {
        let mut logfile = EVENT_TRACE_LOGFILEW::default();
        logfile.LoggerName = windows::core::PWSTR(name_for_thread.as_ptr() as *mut u16);
        // ProcessTraceMode 与 EventRecordCallback 都在匿名联合体里，必须走字段路径
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(etw_callback);
        let handle = unsafe { OpenTraceW(&mut logfile) };
        if handle.Value == u64::MAX || handle.Value == 0 {
            EVENTS_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        // ProcessTrace 阻塞直到会话被 ControlTraceW(STOP) 关闭
        unsafe { ProcessTrace(&[handle], None, None) };
        unsafe { let _ = CloseTrace(handle); }
        EVENTS_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    });

    // 给会话一点时间真正起来；否则紧接着的 poll 拿不到任何事件，
    // 调用方会误以为"事件没发生"。
    std::thread::sleep(std::time::Duration::from_millis(300));
    Ok(())
}

/// 取出已缓存的事件。
///
/// # 返回值
/// 事件列表；订阅未启动时返回具体原因
fn events_poll() -> Result<Vec<EventRecord>, String> {
    if SESSION_HANDLE.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        return Err("事件订阅未启动，请先调用 events.start（需管理员权限）".to_string());
    }
    let mut q = EVENTS.lock().map_err(|_| "事件队列锁不可用".to_string())?;
    let out: Vec<EventRecord> = q.drain(..).collect();
    Ok(out)
}

/// 停止事件订阅。
///
/// # 返回值
/// 无（尽力收尾；失败也不阻塞调用方）
fn events_stop() {
    use windows::Win32::System::Diagnostics::Etw::{
        ControlTraceW, CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_PROPERTIES,
    };
    use windows::core::PCWSTR;

    let session = SESSION_HANDLE.swap(0, std::sync::atomic::Ordering::SeqCst);
    if session == 0 {
        return;
    }
    let name_wide: Vec<u16> = SESSION_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let (mut props, _) = make_properties(SESSION_NAME, 0);
    unsafe {
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE { Value: session },
            PCWSTR(name_wide.as_ptr()),
            props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

// ============================================================================
// D2 用户态栈回溯（RtlCaptureStackBackTrace）
// ============================================================================
//
// 说明：`RtlCaptureStackBackTrace` 只能回溯**调用线程自身**的栈（它是当前上下文的
// 快照）。要回溯**别的进程/线程**的栈，需要挂起目标线程 + GetThreadContext +
// StackWalk64 逐帧读目标内存，那要求 dbghelp 与目标进程的读权限，且容易在
// 目标运行时读到不一致的栈（属已知的困难问题）。
//
// 因此本实现对 `pid == 当前进程` 的情形给出**真实可用的用户态栈**；
// 对其它进程明确返回不支持并说明原因，不返回伪造的帧。

/// 采集本线程的用户态栈。
///
/// # 参数
/// * `skip` - 跳过的帧数（去掉本函数与采集函数自身）
///
/// # 返回值
/// 栈帧列表
fn capture_own_user_stack(skip: u32) -> Result<Vec<StackFrame>, String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn RtlCaptureStackBackTrace(
            framestoskip: u32,
            framestocapture: u32,
            backtrace: *mut *mut c_void,
            backtracehash: *mut u32,
        ) -> u16;
    }
    const MAX: usize = 62;
    let mut frames: [*mut c_void; MAX] = [std::ptr::null_mut(); MAX];
    let n = unsafe {
        RtlCaptureStackBackTrace(skip, MAX as u32, frames.as_mut_ptr(), std::ptr::null_mut())
    } as usize;
    if n == 0 {
        return Err("RtlCaptureStackBackTrace 返回 0 帧".to_string());
    }
    let mut out = Vec::with_capacity(n);
    for f in frames.iter().take(n) {
        let addr = *f as usize;
        // 尝试把地址归到某个已加载模块（用本进程的模块表）
        let (module, offset) = module_of_address(addr);
        out.push(StackFrame {
            address: format!("{addr:x}"),
            module,
            module_offset: offset,
            // 符号名需 dbghelp SymFromAddr，本实现未引入
            symbol: None,
        });
    }
    Ok(out)
}

/// 把地址归属到本进程的某个已加载模块。
///
/// # 参数
/// * `addr` - 指令地址
///
/// # 返回值
/// (模块名, 模块内偏移)；无法归属时均为 None
fn module_of_address(addr: usize) -> (Option<String>, Option<String>) {
    let pid = std::process::id() as i32;
    if let Ok(mods) = modules_of(pid) {
        for m in mods {
            let base = m
                .base_address
                .as_deref()
                .and_then(|s| usize::from_str_radix(s, 16).ok());
            let size = m.size.unwrap_or(0) as usize;
            if let Some(b) = base {
                if addr >= b && addr < b.saturating_add(size) {
                    return (Some(m.name), Some(format!("{:x}", addr - b)));
                }
            }
        }
    }
    (None, None)
}

/// 构造一个"不支持"的信封 JSON。
///
/// # 参数
/// * `op` - 操作名
/// * `why` - 具体原因（必须写清，不能只说"不支持"）
///
/// # 返回值
/// 序列化后的 `Envelope`
fn unsupported_json(op: &str, why: &str) -> String {
    serde_json::to_string(&Envelope::<()>::err(format!("{op}: {why}"))).unwrap_or_default()
}

// ============================================================================
// 以下为 2026-09-30 补齐的 Windows 缺失 op
// ============================================================================

/// `NtQueryInformationProcess` 的信息类：`ProcessBasicInformation`。
const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;

/// 系统模块信息类号（`SystemModuleInformation`）。
const SYSTEM_MODULE_INFORMATION: u32 = 11;

/// x64 上 `PEB.ProcessParameters` 的偏移。
const PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;

/// x64 上 `RTL_USER_PROCESS_PARAMETERS.Environment` 的偏移。
const RPP_ENVIRONMENT_OFFSET: usize = 0x80;

/// x64 上 `RTL_USER_PROCESS_PARAMETERS.EnvironmentSize` 的偏移。
const RPP_ENVIRONMENT_SIZE_OFFSET: usize = 0x3F0;

/// `PROCESS_BASIC_INFORMATION`（x64 布局；只用到 `PebBaseAddress`）。
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ProcessBasicInformation {
    /// 退出状态。
    exit_status: i32,
    /// PEB 地址（x64 下前置 4 字节填充后紧跟指针）。
    peb_base_address: usize,
    /// 亲和性掩码。
    affinity_mask: usize,
    /// 基优先级。
    base_priority: i32,
    /// 唯一进程 ID。
    unique_process_id: usize,
    /// 父进程 ID。
    inherited_from_unique_process_id: usize,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        process_handle: HANDLE,
        process_information_class: u32,
        process_information: *mut c_void,
        process_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn ReadProcessMemory(
        hprocess: HANDLE,
        lpbaseaddress: *const c_void,
        lpbuffer: *mut c_void,
        nsize: usize,
        lpnumberofbytesread: *mut usize,
    ) -> i32;
    fn VirtualQueryEx(
        hprocess: HANDLE,
        lpaddress: *const c_void,
        lpbuffer: *mut MemoryBasicInformation,
        dwlength: usize,
    ) -> usize;
    fn GetMappedFileNameW(
        hprocess: HANDLE,
        lpv: *const c_void,
        lpfilename: *mut u16,
        nsize: u32,
    ) -> u32;
    fn OpenSCManagerW(
        lpmachinename: *const u16,
        lpdatabasename: *const u16,
        dwdesiredaccess: u32,
    ) -> isize;
    fn EnumServicesStatusExW(
        hscmanager: isize,
        infolevel: i32,
        dwservicetype: u32,
        dwservicestate: u32,
        lpbuffer: *mut u8,
        cbbufsize: u32,
        bytesneeded: *mut u32,
        servicesreturned: *mut u32,
        lpresumehandle: *mut u32,
        lpszgroupname: *const u16,
    ) -> i32;
    fn CloseServiceHandle(hscobject: isize) -> i32;
    fn DeviceIoControl(
        hdevice: HANDLE,
        dwiocontrolcode: u32,
        lpinbuffer: *mut c_void,
        ninbuffersize: u32,
        lpoutbuffer: *mut c_void,
        noutbuffersize: u32,
        lpbytesreturned: *mut u32,
        lpoverlapped: *mut c_void,
    ) -> i32;
    fn CreateFileW(
        lpfilename: *const u16,
        dwdesiredaccess: u32,
        dwsharemode: u32,
        lpsecurityattributes: *mut c_void,
        dwcreationdisposition: u32,
        dwflagsandattributes: u32,
        htemplatefile: HANDLE,
    ) -> HANDLE;
    fn EnumDisplayDevicesW(
        lpdevicename: *const u16,
        dwdevnum: u32,
        lpdisplaydevice: *mut DisplayDeviceW,
        dwflags: u32,
    ) -> i32;
}

/// `MEMORY_BASIC_INFORMATION`（x64）。
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct MemoryBasicInformation {
    /// 基址。
    base_address: *mut c_void,
    /// 分配基址。
    allocation_base: *mut c_void,
    /// 分配保护。
    allocation_protect: u32,
    /// 区域大小。
    region_size: usize,
    /// 状态（MEM_COMMIT / MEM_FREE / MEM_RESERVE）。
    state: u32,
    /// 保护。
    protect: u32,
    /// 类型（MEM_IMAGE / MEM_MAPPED / MEM_PRIVATE）。
    mem_type: u32,
}

/// `DISPLAY_DEVICEW`。
#[repr(C)]
#[derive(Clone, Copy)]
struct DisplayDeviceW {
    /// 结构大小。
    cb: u32,
    /// 设备名。
    device_name: [u16; 32],
    /// 设备描述。
    device_string: [u16; 128],
    /// 状态标志。
    state_flags: u32,
    /// 设备 ID。
    device_id: [u16; 128],
    /// 设备键。
    device_key: [u16; 128],
}

impl Default for DisplayDeviceW {
    /// 全零初始化。
    ///
    /// # 返回值
    /// 默认实例
    fn default() -> Self {
        Self {
            cb: std::mem::size_of::<DisplayDeviceW>() as u32,
            device_name: [0; 32],
            device_string: [0; 128],
            state_flags: 0,
            device_id: [0; 128],
            device_key: [0; 128],
        }
    }
}

/// `DISK_PERFORMANCE`（x64 布局）。
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct DiskPerformance {
    /// 累计读字节。
    bytes_read: i64,
    /// 累计写字节。
    bytes_written: i64,
    /// 读耗时（100ns）。
    read_time: i64,
    /// 写耗时（100ns）。
    write_time: i64,
    /// 空闲时间。
    idle_time: i64,
    /// 读次数。
    read_count: u32,
    /// 写次数。
    write_count: u32,
    /// 当前队列深度。
    queue_depth: u32,
    /// 拆分次数。
    split_count: u32,
    /// 查询时刻。
    query_time: i64,
    /// 存储设备号。
    storage_device_number: u32,
    /// 存储管理器名。
    storage_manager_name: [u16; 8],
}

/// 转成 UTF-16 并以 NUL 结尾的缓冲，供 Win32 W 系列 API 使用。
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// C2 环境变量：读取目标进程 PEB 里的环境块。
///
/// 仅支持 x64 目标。WOW64（32 位）目标的结构偏移不同，本实现不猜，直接返回明确原因。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 环境变量列表
fn env_of(pid: i32) -> Result<Vec<EnvVar>, String> {
    use windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
    use windows::Win32::System::Threading::PROCESS_VM_READ;

    let h = open_process(pid, PROCESS_QUERY_INFORMATION | PROCESS_VM_READ).map_err(|e| {
        format!("{e}；读取目标进程环境变量需要 PROCESS_VM_READ，通常要管理员权限")
    })?;

    let result = (|| -> Result<Vec<EnvVar>, String> {
        unsafe {
            // WOW64 目标的 PEB 偏移与 x64 不同，不猜
            let mut is_wow = windows::Win32::Foundation::BOOL(0);
            if IsWow64Process(h, &mut is_wow).is_ok() && is_wow.as_bool() {
                return Err("目标进程是 32 位（WOW64），其 PEB 布局与 x64 不同，本实现不支持".to_string());
            }

            let mut pbi = ProcessBasicInformation::default();
            let mut ret = 0u32;
            let rc = NtQueryInformationProcess(
                h,
                PROCESS_BASIC_INFORMATION_CLASS,
                &mut pbi as *mut _ as *mut c_void,
                std::mem::size_of::<ProcessBasicInformation>() as u32,
                &mut ret,
            );
            if rc != STATUS_SUCCESS || pbi.peb_base_address == 0 {
                return Err(format!("NtQueryInformationProcess(ProcessBasicInformation) 失败: 0x{:08x}", rc as u32));
            }

            let mut read = 0usize;
            let mut params_addr = 0usize;
            if ReadProcessMemory(
                h,
                (pbi.peb_base_address + PEB_PROCESS_PARAMETERS_OFFSET) as *const c_void,
                &mut params_addr as *mut _ as *mut c_void,
                std::mem::size_of::<usize>(),
                &mut read,
            ) == 0
                || params_addr == 0
            {
                return Err("读取 PEB.ProcessParameters 失败".to_string());
            }

            let mut env_addr = 0usize;
            if ReadProcessMemory(
                h,
                (params_addr + RPP_ENVIRONMENT_OFFSET) as *const c_void,
                &mut env_addr as *mut _ as *mut c_void,
                std::mem::size_of::<usize>(),
                &mut read,
            ) == 0
                || env_addr == 0
            {
                return Err("读取 ProcessParameters.Environment 失败".to_string());
            }

            let mut env_size = 0usize;
            if ReadProcessMemory(
                h,
                (params_addr + RPP_ENVIRONMENT_SIZE_OFFSET) as *const c_void,
                &mut env_size as *mut _ as *mut c_void,
                std::mem::size_of::<usize>(),
                &mut read,
            ) == 0
                || env_size == 0
            {
                return Err("读取 ProcessParameters.EnvironmentSize 失败".to_string());
            }
            // 上限保护：异常大的长度会一次性申请巨量内存
            if env_size > 1 << 22 {
                return Err(format!("环境块长度异常（{env_size} 字节），拒绝读取"));
            }

            let mut buf = vec![0u16; env_size / 2];
            if ReadProcessMemory(
                h,
                env_addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                env_size,
                &mut read,
            ) == 0
            {
                return Err("读取环境块失败".to_string());
            }

            let mut out = Vec::new();
            let mut start = 0usize;
            for i in 0..buf.len() {
                if buf[i] == 0 {
                    if i > start {
                        let s = String::from_utf16_lossy(&buf[start..i]);
                        // 只收 "K=V" 形式；环境块里也有以 "=" 开头的伪变量（如 =C:），保留为 key 空值
                        if let Some(eq) = s.find('=') {
                            out.push(EnvVar {
                                key: s[..eq].to_string(),
                                value: s[eq + 1..].to_string(),
                            });
                        }
                    }
                    start = i + 1;
                }
            }
            Ok(out)
        }
    })();

    unsafe { let _ = CloseHandle(h); };
    result
}

/// C6 内存映射：`VirtualQueryEx` 遍历。
///
/// # 参数
/// * `pid` - 目标进程 ID
///
/// # 返回值
/// 映射列表
fn mappings_of(pid: i32) -> Result<Vec<MappingInfo>, String> {
    use windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;

    let h = open_process(pid, PROCESS_QUERY_INFORMATION)
        .map_err(|e| format!("{e}；遍历内存映射需要 PROCESS_QUERY_INFORMATION"))?;
    let mut out = Vec::new();
    unsafe {
        let mut addr: usize = 0;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 200_000 {
                break; // 防不收敛
            }
            let mut mbi = MemoryBasicInformation::default();
            let n = VirtualQueryEx(
                h,
                addr as *const c_void,
                &mut mbi,
                std::mem::size_of::<MemoryBasicInformation>(),
            );
            if n == 0 {
                break;
            }
            let base = mbi.base_address as usize;
            let size = mbi.region_size as usize;
            if size == 0 {
                break;
            }
            // 只报已提交的区域；空闲/保留区没有意义且条目极多
            if mbi.state == 0x1000 {
                let path = {
                    let mut buf = [0u16; 1024];
                    let len = GetMappedFileNameW(h, base as *const c_void, buf.as_mut_ptr(), buf.len() as u32);
                    if len > 0 {
                        Some(String::from_utf16_lossy(&buf[..len as usize]))
                    } else {
                        None
                    }
                };
                out.push(MappingInfo {
                    base_address: format!("{:x}", base),
                    size: size as u64,
                    protection: protection_str(mbi.protect),
                    kind: match mbi.mem_type {
                        0x1000000 => "image",
                        0x40000 => "mapped",
                        0x20000 => "private",
                        _ => "unknown",
                    }
                    .to_string(),
                    path,
                });
            }
            addr = base.saturating_add(size);
            if addr == 0 {
                break;
            }
        }
        let _ = CloseHandle(h);
    }
    if out.is_empty() {
        return Err(format!(
            "进程 {pid} 未返回任何已提交内存区域；可能需要管理员权限或进程已退出"
        ));
    }
    Ok(out)
}

/// 把 `MEMORY_BASIC_INFORMATION.Protect` 转成简写权限串。
///
/// # 参数
/// * `protect` - 保护标志
///
/// # 返回值
/// 形如 `r-x` / `rw-` 的串
fn protection_str(protect: u32) -> String {
    let base = protect & 0xFF;
    match base {
        0x01 => "---",
        0x02 => "r--",
        0x04 => "rw-",
        0x08 => "--w",
        0x10 => "r-x",
        0x20 => "r-x",
        0x40 => "rwx",
        0x80 => "rwx",
        _ => "???",
    }
    .to_string()
}

/// C7 套接字 / 连接：全系统 TCP/UDP 表（按 pid 过滤）。
///
/// # 参数
/// * `pid` - 目标进程 ID；None 表示全系统
///
/// # 返回值
/// 连接列表
fn sockets_of(pid: Option<i32>) -> Result<Vec<SocketInfo>, String> {
    let mut out = Vec::new();
    out.extend(tcp_sockets(pid)?);
    out.extend(udp_sockets(pid)?);
    if out.is_empty() {
        return Err(match pid {
            Some(p) => format!("进程 {p} 没有 TCP/UDP 连接"),
            None => "未取到任何 TCP/UDP 连接".to_string(),
        });
    }
    Ok(out)
}

/// IPv4 网络序整型转点分十进制。
///
/// # 参数
/// * `addr` - 网络序地址
///
/// # 返回值
/// 点分十进制串
fn ipv4_str(addr: u32) -> String {
    let b = addr.to_ne_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// 端口（网络序低 16 位）转主机序。
///
/// # 参数
/// * `port` - 网络序端口
///
/// # 返回值
/// 主机序端口
fn port_of(port: u32) -> u16 {
    u16::from_be((port & 0xFFFF) as u16)
}

/// 枚举 TCP 连接。
///
/// # 参数
/// * `pid` - 目标进程 ID；None 表示全系统
///
/// # 返回值
/// 连接列表
fn tcp_sockets(pid: Option<i32>) -> Result<Vec<SocketInfo>, String> {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TcpRow {
        state: u32,
        local_addr: u32,
        local_port: u32,
        remote_addr: u32,
        remote_port: u32,
        owning_pid: u32,
    }
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            ptcptable: *mut c_void,
            pdwsize: *mut u32,
            border: i32,
            ulaf: u32,
            tableclass: i32,
            reserved: u32,
        ) -> u32;
    }
    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_ALL: i32 = 5;

    let mut size = 0u32;
    unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0) };
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; size as usize];
    let rc = unsafe {
        GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if rc != 0 {
        return Err(format!("GetExtendedTcpTable 失败: {rc}"));
    }
    let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let row_size = std::mem::size_of::<TcpRow>();
    let mut out = Vec::new();
    for i in 0..count {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        let row: TcpRow = unsafe { std::ptr::read_unaligned(buf[off..].as_ptr() as *const TcpRow) };
        if let Some(p) = pid {
            if row.owning_pid as i32 != p {
                continue;
            }
        }
        out.push(SocketInfo {
            protocol: "tcp".to_string(),
            local: format!("{}:{}", ipv4_str(row.local_addr), port_of(row.local_port)),
            remote: if row.remote_addr == 0 {
                None
            } else {
                Some(format!("{}:{}", ipv4_str(row.remote_addr), port_of(row.remote_port)))
            },
            state: tcp_state(row.state),
            pid: Some(row.owning_pid as i32),
            inode: None,
        });
    }
    Ok(out)
}

/// 枚举 UDP 端点。
///
/// # 参数
/// * `pid` - 目标进程 ID；None 表示全系统
///
/// # 返回值
/// 端点列表
fn udp_sockets(pid: Option<i32>) -> Result<Vec<SocketInfo>, String> {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct UdpRow {
        local_addr: u32,
        local_port: u32,
        owning_pid: u32,
    }
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedUdpTable(
            pudptable: *mut c_void,
            pdwsize: *mut u32,
            border: i32,
            ulaf: u32,
            tableclass: i32,
            reserved: u32,
        ) -> u32;
    }
    const AF_INET: u32 = 2;
    const UDP_TABLE_OWNER_PID: i32 = 1;

    let mut size = 0u32;
    unsafe { GetExtendedUdpTable(std::ptr::null_mut(), &mut size, 0, AF_INET, UDP_TABLE_OWNER_PID, 0) };
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; size as usize];
    let rc = unsafe {
        GetExtendedUdpTable(
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            0,
            AF_INET,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if rc != 0 {
        return Err(format!("GetExtendedUdpTable 失败: {rc}"));
    }
    let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let row_size = std::mem::size_of::<UdpRow>();
    let mut out = Vec::new();
    for i in 0..count {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        let row: UdpRow = unsafe { std::ptr::read_unaligned(buf[..].as_ptr().add(off) as *const UdpRow) };
        if let Some(p) = pid {
            if row.owning_pid as i32 != p {
                continue;
            }
        }
        out.push(SocketInfo {
            protocol: "udp".to_string(),
            local: format!("{}:{}", ipv4_str(row.local_addr), port_of(row.local_port)),
            remote: None,
            state: "NONE".to_string(),
            pid: Some(row.owning_pid as i32),
            inode: None,
        });
    }
    Ok(out)
}

/// TCP 状态码转名字。
///
/// # 参数
/// * `s` - MIB_TCP_STATE 值
///
/// # 返回值
/// 状态名
fn tcp_state(s: u32) -> String {
    match s {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RCVD",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT1",
        7 => "FIN_WAIT2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
    .to_string()
}

/// A8 每磁盘 IO：`IOCTL_DISK_PERFORMANCE`。
///
/// # 返回值
/// 磁盘 IO 列表
fn disk_io_of() -> Result<Vec<DiskIo>, String> {
    const IOCTL_DISK_PERFORMANCE: u32 = 0x0007_0020;
    let mut out = Vec::new();
    for i in 0..32u32 {
        let name = to_wide(&format!("\\\\.\\PhysicalDrive{i}"));
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x8000_0000, // GENERIC_READ
                // FILE_SHARE_READ | FILE_SHARE_WRITE：独占打开会被系统占用挡住
                0x0000_0001 | 0x0000_0002,
                std::ptr::null_mut(),
                3, // OPEN_EXISTING
                0,
                HANDLE::default(),
            )
        };
        if h.is_invalid() {
            continue;
        }
        let mut perf = DiskPerformance::default();
        let mut ret = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                h,
                IOCTL_DISK_PERFORMANCE,
                std::ptr::null_mut(),
                0,
                &mut perf as *mut _ as *mut c_void,
                std::mem::size_of::<DiskPerformance>() as u32,
                &mut ret,
                std::ptr::null_mut(),
            )
        };
        unsafe { let _ = CloseHandle(h); };
        if ok == 0 {
            continue;
        }
        out.push(DiskIo {
            name: format!("PhysicalDrive{i}"),
            read_bytes: perf.bytes_read.max(0) as u64,
            written_bytes: perf.bytes_written.max(0) as u64,
            read_count: perf.read_count as u64,
            write_count: perf.write_count as u64,
            queue_depth: Some(perf.queue_depth as u64),
        });
    }
    if out.is_empty() {
        return Err("未能打开任何 PhysicalDrive 查询性能；通常需要管理员权限".to_string());
    }
    Ok(out)
}

/// D4 内核模块 / 驱动：`NtQuerySystemInformation(SystemModuleInformation)`。
///
/// # 返回值
/// 内核模块列表
fn kernel_modules_of() -> Result<Vec<KernelModuleInfo>, String> {
    let mut buf = vec![0u8; 1 << 20];
    let mut len = buf.len() as u32;
    let mut rc = 0i32;
    for _ in 0..4 {
        let mut ret = 0u32;
        rc = unsafe {
            NtQuerySystemInformation(
                SYSTEM_MODULE_INFORMATION,
                buf.as_mut_ptr() as *mut c_void,
                len,
                &mut ret,
            )
        };
        if rc == STATUS_SUCCESS {
            break;
        }
        if rc == 0xC000_0004u32 as i32 && ret > len && ret < (1 << 28) {
            len = ret;
            buf = vec![0u8; len as usize];
            continue;
        }
        break;
    }
    if rc != STATUS_SUCCESS {
        return Err(format!(
            "NtQuerySystemInformation(SystemModuleInformation) 失败: 0x{:08x}",
            rc as u32
        ));
    }

    // RTL_PROCESS_MODULES { ULONG NumberOfModules; RTL_PROCESS_MODULE_INFORMATION Modules[] }
    // x64 上每项 296 字节：Section(8) MappedBase(8) ImageBase(8) ImageSize(4) Flags(4)
    //   LoadOrderIndex(2) InitOrderIndex(2) LoadCount(2) OffsetToFileName(2) FullPathName(256)
    const MOD_HDR: usize = 8;
    const MOD_ENTRY: usize = 296;
    const FULL_PATH: usize = 8 + 8 + 8 + 4 + 4 + 2 + 2 + 2 + 2;
    let n = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let mut out = Vec::new();
    for i in 0..n.min(4096) {
        let off = MOD_HDR + i * MOD_ENTRY;
        if off + MOD_ENTRY > buf.len() {
            break;
        }
        let image_base = usize::from_ne_bytes([
            buf[off + 16], buf[off + 17], buf[off + 18], buf[off + 19],
            buf[off + 20], buf[off + 21], buf[off + 22], buf[off + 23],
        ]);
        let image_size = u32::from_ne_bytes([buf[off + 24], buf[off + 25], buf[off + 26], buf[off + 27]]);
        let offset_to_name = u16::from_ne_bytes([buf[off + 34], buf[off + 35]]) as usize;
        let path_start = off + FULL_PATH + offset_to_name;
        if path_start >= buf.len() {
            continue;
        }
        let end = buf[path_start..]
            .iter()
            .position(|c| *c == 0)
            .map(|p| path_start + p)
            .unwrap_or(path_start);
        let path = String::from_utf8_lossy(&buf[path_start..end]).to_string();
        let name = path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&path)
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(KernelModuleInfo {
            name,
            path: Some(path),
            base_address: Some(format!("{image_base:x}")),
            size: Some(image_size as u64),
        });
    }
    if out.is_empty() {
        return Err("SystemModuleInformation 返回 0 个模块".to_string());
    }
    Ok(out)
}

/// D6 服务列表：SCM 枚举。
///
/// # 返回值
/// 服务列表
fn services_of() -> Result<Vec<ServiceInfo>, String> {
    const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x0004;
    const SERVICE_WIN32: u32 = 0x30;
    const SC_ENUM_PROCESS_INFO: i32 = 0;

    let h = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ENUMERATE_SERVICE) };
    if h == 0 {
        return Err("OpenSCManagerW 失败；通常需要管理员权限".to_string());
    }
    let mut size = 0u32;
    let mut needed = 0u32;
    let mut returned = 0u32;
    let mut resume = 0u32;
    // 第一次拿所需缓冲大小（预期返回 ERROR_MORE_DATA）
    unsafe {
        EnumServicesStatusExW(
            h,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            0x0000_0003, // SERVICE_STATE_ALL
            std::ptr::null_mut(),
            0,
            &mut needed,
            &mut returned,
            &mut resume,
            std::ptr::null(),
        );
    }
    if needed == 0 {
        unsafe { CloseServiceHandle(h); }
        return Err("EnumServicesStatusExW 未返回所需缓冲大小".to_string());
    }
    // 申报上限，异常大时拒绝（服务数上千时该缓冲也就几百 KB）
    if needed > 1 << 26 {
        unsafe { CloseServiceHandle(h); }
        return Err(format!("服务枚举缓冲异常大（{needed} 字节），拒绝分配"));
    }
    size = needed;
    let mut buf = vec![0u8; size as usize];
    let ok = unsafe {
        EnumServicesStatusExW(
            h,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            0x0000_0003,
            buf.as_mut_ptr(),
            size,
            &mut needed,
            &mut returned,
            &mut resume,
            std::ptr::null(),
        )
    };
    unsafe { CloseServiceHandle(h); };
    if ok == 0 {
        return Err("EnumServicesStatusExW 失败".to_string());
    }

    // ENUM_SERVICE_STATUS_PROCESSW（x64）：
    //   LPWSTR lpServiceName(0) LPWSTR lpDisplayName(8) SERVICE_STATUS_PROCESS ServiceStatus(16)
    // SERVICE_STATUS_PROCESS 内部偏移：
    //   dwServiceType(0) dwCurrentState(4) dwControlsAccepted(8) dwWin32ExitCode(12)
    //   dwServiceSpecificExitCode(16) dwCheckPoint(20) dwWaitHint(24) dwProcessId(28)
    //   dwServiceFlags(32)  —— 共 36 字节，因 8 字节对齐补到 40
    // 结构总长 8+8+40 = 56
    //
    // 曾把 dwCurrentState 读成 off+16（其实是 dwServiceType=0x30），
    // 把 dwProcessId 读成 off+48（其实是 dwServiceFlags），
    // 结果 297 个服务的状态全部落到 "unknown"、pid 全部错位。
    // 编译不会报错，只有真跑并断言"至少有一个 running"才发现。
    const ENTRY: usize = 56;
    const STATE_OFF: usize = 20;
    const PID_OFF: usize = 44;
    let mut out = Vec::new();
    for i in 0..(returned as usize).min(8192) {
        let off = i * ENTRY;
        if off + ENTRY > buf.len() {
            break;
        }
        let name_ptr = usize::from_ne_bytes([
            buf[off], buf[off + 1], buf[off + 2], buf[off + 3],
            buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7],
        ]);
        let disp_ptr = usize::from_ne_bytes([
            buf[off + 8], buf[off + 9], buf[off + 10], buf[off + 11],
            buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15],
        ]);
        let state = u32::from_ne_bytes([
            buf[off + STATE_OFF], buf[off + STATE_OFF + 1],
            buf[off + STATE_OFF + 2], buf[off + STATE_OFF + 3],
        ]);
        let pid = u32::from_ne_bytes([
            buf[off + PID_OFF], buf[off + PID_OFF + 1],
            buf[off + PID_OFF + 2], buf[off + PID_OFF + 3],
        ]);
        let name = unsafe { read_wide_ptr(name_ptr) }.unwrap_or_default();
        let display = unsafe { read_wide_ptr(disp_ptr) }.unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push(ServiceInfo {
            name,
            display_name: display,
            state: match state {
                1 => "stopped",
                2 => "start_pending",
                3 => "stop_pending",
                4 => "running",
                5 => "continue_pending",
                6 => "pause_pending",
                7 => "paused",
                _ => "unknown",
            }
            .to_string(),
            // 启动类型与账号需另调 QueryServiceConfigW；本实现未做，留空而不编造
            start_type: "unknown".to_string(),
            account: None,
            binary_path: None,
            is_driver: None,
            pid: if pid == 0 { None } else { Some(pid as i32) },
        });
    }
    if out.is_empty() {
        return Err("EnumServicesStatusExW 返回 0 个服务".to_string());
    }
    Ok(out)
}

/// 读一个由本机分配的 UTF-16 字符串指针。
///
/// # 安全性
/// `ptr` 必须指向以 NUL 结尾的合法 UTF-16 串，或为 0。
unsafe fn read_wide_ptr(ptr: usize) -> Option<String> {
    if ptr == 0 {
        return None;
    }
    let p = ptr as *const u16;
    let mut len = 0usize;
    // 与 handles_of 同样给上界：不信任长度字段，最多扫 4096 个码元
    while len < 4096 && *p.add(len) != 0 {
        len += 1;
    }
    if len == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
}

/// A10 GPU：枚举显示适配器。
///
/// 只返回**确实拿得到**的字段（适配器名、设备 ID）。使用率/显存/温度/功耗需要
/// `D3DKMTQueryStatistics`（其结构含大联合体，偏移写错会内存损坏），本实现不做，
/// 相应字段一律为 null 而不是 0。
///
/// # 返回值
/// 适配器列表
fn gpus_of() -> Result<Vec<GpuInfo>, String> {
    let mut out = Vec::new();
    for i in 0..16u32 {
        let mut dd = DisplayDeviceW::default();
        let ok = unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) };
        if ok == 0 {
            break;
        }
        let name = String::from_utf16_lossy(
            &dd.device_string[..dd.device_string.iter().position(|c| *c == 0).unwrap_or(0)],
        );
        let id = String::from_utf16_lossy(
            &dd.device_id[..dd.device_id.iter().position(|c| *c == 0).unwrap_or(0)],
        );
        if name.is_empty() {
            continue;
        }
        let vendor = if id.contains("VEN_10DE") {
            "nvidia"
        } else if id.contains("VEN_1002") || id.contains("VEN_1022") {
            "amd"
        } else if id.contains("VEN_8086") {
            "intel"
        } else if id.contains("VEN_106B") || id.contains("APPLE") {
            "apple"
        } else {
            "unknown"
        };
        out.push(GpuInfo {
            vendor: vendor.to_string(),
            name,
            memory_total: None,
            memory_used: None,
            usage: None,
            temperature_c: None,
            power_w: None,
            driver_version: None,
        });
    }
    if out.is_empty() {
        return Err("EnumDisplayDevicesW 未枚举到显示适配器".to_string());
    }
    Ok(out)
}

