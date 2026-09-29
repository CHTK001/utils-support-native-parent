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
    unsupported, ActionKind, ActionResult, CredentialInfo, Envelope, HandleInfo, ModuleInfo,
    PrivilegeInfo, ProcessDetail, ThreadInfo,
};
use crate::PLATFORM;

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, MAX_PATH};
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
        "process.detail" => wrap(need_pid().and_then(|pid| {
            let mut found = common::sysinfo_processes()
                .into_iter()
                .find(|p| p.pid == pid)
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

        // ---------- 明确不支持（原因写清，不用空集合冒充）----------
        "process.env" => unsupported_json(
            "process.env",
            "读取目标进程环境变量需要 PROCESS_VM_READ 权限并解析其 PEB，本实现未做",
        ),
        "process.mappings" => unsupported_json(
            "process.mappings",
            "内存映射遍历需 VirtualQueryEx 逐区读取，本实现未做",
        ),
        "process.stack" => unsupported_json(
            "process.stack",
            "用户态栈回溯需 dbghelp StackWalk64；内核态栈需 ETW + 管理员权限。本实现均未做",
        ),
        "socket.list" => unsupported_json(
            "socket.list",
            "需 GetExtendedTcpTable/GetExtendedUdpTable（iphlpapi），本实现未做",
        ),
        "service.list" => unsupported_json(
            "service.list",
            "需 SCM EnumServicesStatusExW + QueryServiceConfigW，本实现未做",
        ),
        "kernel.modules" => unsupported_json(
            "kernel.modules",
            "NtQuerySystemInformation(SystemModuleInformation) 本实现未做",
        ),
        "disk.io" => unsupported_json(
            "disk.io",
            "每磁盘 IO 需 DeviceIoControl(IOCTL_DISK_PERFORMANCE) 或性能计数器，本实现未做",
        ),
        "gpu.list" => unsupported_json(
            "gpu.list",
            "需 D3DKMTQueryStatistics（用户态可得，无需驱动），本实现未做",
        ),
        "sensor.list" => unsupported_json("sensor.list", "需 WMI 查询，本实现未做"),
        "memory.modules" => unsupported_json(
            "memory.modules",
            "需 WMI Win32_PhysicalMemory 查询，本实现未做",
        ),
        "events.start" | "events.poll" | "events.stop" => unsupported_json(
            op,
            "事件驱动需 ETW（Microsoft-Windows-Kernel-Process 或 NT Kernel Logger）+ 管理员权限，本实现未做",
        ),

        other => serde_json::to_string(&Envelope::<()>::err(unsupported(other, PLATFORM)))
            .unwrap_or_default(),
    }
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
