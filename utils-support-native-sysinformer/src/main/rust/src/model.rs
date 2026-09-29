//! 全部数据模型 —— 本模块的**公共契约**。
//!
//! 平台实现（`platform_windows.rs` / `platform_linux.rs` / `platform_macos.rs`）
//! 与跨平台基线（`common.rs`）都必须返回这里定义的类型。改本文件等于改接口，
//! 必须同步所有实现，因此它由单人维护，不参与并行分工。
//!
//! 字段命名统一 snake_case，全部 `Serialize`，最终由 `lib.rs` 序列化成 JSON 返回给
//! Java 侧。**新增字段一律用 `Option<T>`**：不同平台能力不等价，缺失必须有显式表达，
//! 不能用默认值伪装成"采集到了 0"。

use serde::{Deserialize, Serialize};

// ============================================================================
// A. 系统级指标
// ============================================================================

/// A1 单个逻辑 CPU 核。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCore {
    /// 核序号，从 0 开始。
    pub id: u32,
    /// 使用率百分比，0~100。
    pub usage: f32,
    /// 当前频率（MHz）。平台取不到时为空。
    pub frequency_mhz: Option<u64>,
    /// 型号串。
    pub brand: String,
}

/// A2 CPU 汇总。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuSummary {
    /// 逻辑核数。
    pub logical_count: u32,
    /// 物理核数。取不到时为空。
    pub physical_count: Option<u32>,
    /// 整体使用率百分比。
    pub usage: f32,
    /// 型号串。
    pub brand: String,
    /// 架构，如 x86_64 / aarch64。
    pub arch: String,
}

/// A3 负载。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadAverage {
    /// 1 分钟。
    pub one: f64,
    /// 5 分钟。
    pub five: f64,
    /// 15 分钟。
    pub fifteen: f64,
    /// Windows 无原生负载；true 表示该值是由 CPU 队列近似出来的。
    pub approximated: bool,
}

/// A4 / A5 内存。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    /// 物理总量（字节）。
    pub total: u64,
    /// 已用（字节）。
    pub used: u64,
    /// 空闲（字节）。
    pub free: u64,
    /// 可用（含可回收缓存，字节）。
    pub available: u64,
    /// 缓存（字节），平台无此概念时为空。
    pub cached: Option<u64>,
    /// 缓冲区（字节），平台无此概念时为空。
    pub buffers: Option<u64>,
    /// 物理内存条列表（A5）。
    pub modules: Vec<MemoryModule>,
}

/// A5 物理内存条。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryModule {
    /// 插槽标识。
    pub slot: String,
    /// 容量（字节）。
    pub capacity: u64,
    /// 类型，如 DDR4 / DDR5。
    pub memory_type: Option<String>,
    /// 速度（MHz）。
    pub speed_mhz: Option<u32>,
    /// 厂商。
    pub manufacturer: Option<String>,
    /// 序列号。
    pub serial: Option<String>,
}

/// A6 Swap。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapInfo {
    /// 总量（字节）。
    pub total: u64,
    /// 已用（字节）。
    pub used: u64,
    /// 空闲（字节）。
    pub free: u64,
}

/// A7 磁盘分区。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskPartition {
    /// 设备名。
    pub name: String,
    /// 挂载点。
    pub mount_point: String,
    /// 总量（字节）。
    pub total: u64,
    /// 已用（字节）。
    pub used: u64,
    /// 可用（字节）。
    pub available: u64,
    /// 文件系统类型。
    pub file_system: String,
    /// 是否可移动介质。
    pub removable: Option<bool>,
    /// 介质类型：ssd / hdd / unknown。
    pub kind: Option<String>,
}

/// A8 每磁盘 IO。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskIo {
    /// 设备名。
    pub name: String,
    /// 累计读字节。
    pub read_bytes: u64,
    /// 累计写字节。
    pub written_bytes: u64,
    /// 累计读次数。
    pub read_count: u64,
    /// 累计写次数。
    pub write_count: u64,
    /// 排队深度，平台无此概念时为空。
    pub queue_depth: Option<u64>,
}

/// A9 网络接口。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// 接口名。
    pub name: String,
    /// 累计接收字节。
    pub received_bytes: u64,
    /// 累计发送字节。
    pub transmitted_bytes: u64,
    /// 累计接收包数。
    pub received_packets: u64,
    /// 累计发送包数。
    pub transmitted_packets: u64,
    /// 接收错误数。
    pub errors_in: u64,
    /// 发送错误数。
    pub errors_out: u64,
    /// 接收丢弃数。
    pub drops_in: Option<u64>,
    /// 发送丢弃数。
    pub drops_out: Option<u64>,
    /// MTU。
    pub mtu: Option<u32>,
    /// MAC 地址。
    pub mac: Option<String>,
    /// IPv4/IPv6 地址列表。
    pub addresses: Vec<String>,
}

/// A10 GPU。多厂商，字段大面积可空。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// 厂商：nvidia / amd / intel / apple / unknown。
    pub vendor: String,
    /// 型号。
    pub name: String,
    /// 显存总量（字节）。
    pub memory_total: Option<u64>,
    /// 显存已用（字节）。
    pub memory_used: Option<u64>,
    /// 使用率百分比。
    pub usage: Option<f32>,
    /// 温度（摄氏度）。
    pub temperature_c: Option<f32>,
    /// 功耗（瓦）。
    pub power_w: Option<f32>,
    /// 驱动版本。
    pub driver_version: Option<String>,
}

/// A11 硬件传感器。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorInfo {
    /// 传感器名。
    pub name: String,
    /// 类别：temperature / fan / voltage。
    pub kind: String,
    /// 数值。
    pub value: f32,
    /// 单位。
    pub unit: String,
}

/// A12 电池。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    /// 设备名。
    pub name: String,
    /// 电量百分比。
    pub percentage: Option<f32>,
    /// 状态：charging / discharging / full / unknown。
    pub state: String,
    /// 剩余时间（秒）。
    pub time_to_empty_sec: Option<u64>,
    /// 充满剩余时间（秒）。
    pub time_to_full_sec: Option<u64>,
}

/// A13 系统时间线。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemTimeline {
    /// 启动时刻的 Unix 毫秒时间戳。
    pub boot_time_ms: Option<i64>,
    /// 运行时长（秒）。
    pub uptime_sec: u64,
}

/// A14 主机信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    /// 主机名。
    pub hostname: String,
    /// 操作系统名。
    pub os_name: String,
    /// 操作系统版本。
    pub os_version: String,
    /// 内核版本。
    pub kernel_version: String,
    /// 架构。
    pub arch: String,
    /// 时区偏移（分钟）。
    pub timezone_offset_min: Option<i32>,
    /// 当前用户。
    pub current_user: Option<String>,
}

// ============================================================================
// B. 进程级（采样路）
// ============================================================================

/// B1~B9 单个进程的完整信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDetail {
    /// 进程 ID。
    pub pid: i32,
    /// 父进程 ID。取不到时为空。
    pub ppid: Option<i32>,
    /// 进程名。
    pub name: String,
    /// 会话 ID（Windows）/ 会话（Unix）。
    pub session_id: Option<u32>,
    /// 所属用户。
    pub user: Option<String>,
    /// 用户 ID。
    pub uid: Option<i32>,
    /// 组 ID。
    pub gid: Option<i32>,
    /// 状态，如 running / sleeping / zombie / stopped。
    pub status: String,
    /// 优先级数值。
    pub priority: Option<i32>,
    /// 调度类/优先级类别，如 normal / high / realtime。
    pub priority_class: Option<String>,
    /// 启动时刻的 Unix 毫秒时间戳。
    pub start_time_ms: Option<i64>,
    /// 运行时长（秒）。
    pub run_time_sec: Option<u64>,
    /// 是否 32 位进程运行在 64 位系统上（Windows WOW64）。
    pub is_wow64: Option<bool>,
    /// 是否以提升权限运行。
    pub is_elevated: Option<bool>,
    /// 是否受保护进程（如 PPL）。
    pub is_protected: Option<bool>,
    /// CPU 使用率百分比。
    pub cpu_usage: f32,
    /// 常驻内存（字节）。
    pub rss: u64,
    /// 虚拟内存（字节）。
    pub virtual_memory: u64,
    /// 私有工作集（字节），Windows 概念。
    pub private_bytes: Option<u64>,
    /// 共享内存（字节）。
    pub shared_bytes: Option<u64>,
    /// 线程数。
    pub thread_count: u32,
    /// 句柄数 / fd 数。
    pub handle_count: Option<u32>,
    /// 累计读字节（B6）。
    pub io_read_bytes: Option<u64>,
    /// 累计写字节。
    pub io_written_bytes: Option<u64>,
    /// 累计读次数。
    pub io_read_count: Option<u64>,
    /// 累计写次数。
    pub io_write_count: Option<u64>,
    /// 完整命令行（B7）。
    pub command_line: Option<String>,
    /// 已解析的命令行参数数组。
    pub args: Vec<String>,
    /// 可执行文件路径（B8）。
    pub exe_path: Option<String>,
    /// 工作目录。
    pub cwd: Option<String>,
    /// 根目录。
    pub root_dir: Option<String>,
    /// 可执行文件签名摘要（D5）。
    pub signature: Option<SignatureInfo>,
}

/// B4 进程树节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTreeNode {
    /// 进程 ID。
    pub pid: i32,
    /// 父进程 ID。
    pub ppid: Option<i32>,
    /// 进程名。
    pub name: String,
    /// 子节点。
    pub children: Vec<ProcessTreeNode>,
}

// ============================================================================
// C. 进程深度内省（按需路）
// ============================================================================

/// C1 线程信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadInfo {
    /// 线程 ID。
    pub tid: i64,
    /// 所属进程 ID。
    pub pid: i32,
    /// 状态。
    pub status: Option<String>,
    /// 优先级数值。
    pub priority: Option<i32>,
    /// 用户态 CPU 时间（毫秒）。
    pub user_time_ms: Option<u64>,
    /// 内核态 CPU 时间（毫秒）。
    pub kernel_time_ms: Option<u64>,
    /// 线程起始地址（十六进制字符串）。
    pub start_address: Option<String>,
    /// 栈基址（十六进制字符串）。
    pub stack_base: Option<String>,
    /// 等待原因。
    pub wait_reason: Option<String>,
    /// 线程名（Windows 10 起可设）。
    pub name: Option<String>,
}

/// C2 环境变量。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    /// 变量名。
    pub key: String,
    /// 变量值。
    pub value: String,
}

/// C3 句柄 / fd。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleInfo {
    /// 句柄值 / fd 号。
    pub id: String,
    /// 类型：file / directory / socket / pipe / registry / event / mutex / thread / process / unknown。
    pub kind: String,
    /// 名称或目标路径。
    pub name: Option<String>,
    /// 访问权限掩码/权限串。
    pub access: Option<String>,
    /// 引用计数（Windows 概念，Unix 为空）。
    pub ref_count: Option<u32>,
}

/// C4 已加载模块。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// 模块名。
    pub name: String,
    /// 完整路径。
    pub path: Option<String>,
    /// 基址（十六进制字符串）。
    pub base_address: Option<String>,
    /// 大小（字节）。
    pub size: Option<u64>,
    /// 版本号。
    pub version: Option<String>,
    /// 厂商。
    pub company: Option<String>,
    /// 描述。
    pub description: Option<String>,
    /// 签名摘要。
    pub signature: Option<SignatureInfo>,
}

/// C5 凭据 / 令牌。三平台语义不同，故字段多为可空。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialInfo {
    /// Windows：令牌所属用户 SID；Unix：用户名。
    pub owner: Option<String>,
    /// 令牌类型：primary / impersonation（Windows）。
    pub token_type: Option<String>,
    /// 模拟级别（Windows）。
    pub impersonation_level: Option<String>,
    /// 完整性级别：low / medium / high / system（Windows）。
    pub integrity_level: Option<String>,
    /// 是否提升（Windows）。
    pub elevated: Option<bool>,
    /// SID 列表 / 组列表。
    pub groups: Vec<String>,
    /// 特权列表及状态（Windows）。
    pub privileges: Vec<PrivilegeInfo>,
    /// Unix capabilities（十六进制/名称列表）。
    pub capabilities: Vec<String>,
    /// seccomp 状态（Linux）。
    pub seccomp: Option<String>,
    /// no_new_privs（Linux）。
    pub no_new_privs: Option<bool>,
    /// AppArmor / SELinux 标签（Linux）。
    pub security_label: Option<String>,
    /// entitlements（macOS）。
    pub entitlements: Vec<String>,
}

/// C5 单条特权。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivilegeInfo {
    /// 特权名，如 SeDebugPrivilege。
    pub name: String,
    /// 是否已启用。
    pub enabled: bool,
}

/// C6 内存映射。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingInfo {
    /// 起始地址（十六进制字符串）。
    pub base_address: String,
    /// 大小（字节）。
    pub size: u64,
    /// 权限串，如 r-x / rw-。
    pub protection: String,
    /// 类型：image / mapped / private / stack / heap / unknown。
    pub kind: String,
    /// 映射的文件路径。
    pub path: Option<String>,
}

/// C7 套接字 / 网络连接。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketInfo {
    /// 协议：tcp / tcp6 / udp / udp6 / unix。
    pub protocol: String,
    /// 本地地址:端口。
    pub local: String,
    /// 远端地址:端口，监听态为空。
    pub remote: Option<String>,
    /// 连接状态，如 LISTEN / ESTABLISHED。
    pub state: String,
    /// 所属进程 ID，取不到时为空。
    pub pid: Option<i32>,
    /// inode（Unix）。
    pub inode: Option<u64>,
}

/// D5 签名摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureInfo {
    /// 是否已签名。
    pub signed: bool,
    /// 签名是否有效。
    pub valid: Option<bool>,
    /// 签发者。
    pub signer: Option<String>,
    /// 证书主题。
    pub subject: Option<String>,
    /// 签名时间（Unix 毫秒）。
    pub timestamp_ms: Option<i64>,
    /// 校验失败原因。
    pub error: Option<String>,
}

// ============================================================================
// D. 高级功能
// ============================================================================

/// D1 事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    /// 进程创建。
    ProcessStart,
    /// 进程退出。
    ProcessStop,
    /// 线程创建。
    ThreadStart,
    /// 线程退出。
    ThreadStop,
    /// 镜像（DLL/so）加载。
    ImageLoad,
    /// 镜像卸载。
    ImageUnload,
    /// 网络连接建立。
    NetworkConnect,
}

/// D1 单条事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    /// 事件类型。
    pub kind: EventKind,
    /// 发生时刻的 Unix 毫秒时间戳。
    pub timestamp_ms: i64,
    /// 主进程 ID。
    pub pid: i32,
    /// 父进程 ID（进程创建事件）。
    pub ppid: Option<i32>,
    /// 相关 ID：线程事件为 TID，镜像事件为基址。
    pub related_id: Option<i64>,
    /// 名称：进程名 / 线程名 / 镜像路径 / 连接对端。
    pub name: Option<String>,
    /// 附加信息。
    pub detail: Option<String>,
}

/// D2 栈帧。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    /// 指令地址（十六进制字符串）。
    pub address: String,
    /// 所属模块。
    pub module: Option<String>,
    /// 模块内偏移（十六进制字符串）。
    pub module_offset: Option<String>,
    /// 解析出的符号名。
    pub symbol: Option<String>,
}

/// D2 栈回溯结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackTrace {
    /// 所属进程 ID。
    pub pid: i32,
    /// 线程 ID。
    pub tid: Option<i64>,
    /// 是否为内核态栈。
    pub kernel: bool,
    /// 栈帧列表，最内层在前。
    pub frames: Vec<StackFrame>,
    /// 取不到时的原因。
    pub error: Option<String>,
}

/// D3 可由本模块执行的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    /// 终止进程。
    TerminateProcess,
    /// 挂起进程。
    SuspendProcess,
    /// 恢复进程。
    ResumeProcess,
    /// 挂起线程。
    SuspendThread,
    /// 恢复线程。
    ResumeThread,
    /// 设置优先级。
    SetPriority,
    /// 设置 CPU 亲和性。
    SetAffinity,
    /// 关闭句柄 / fd。
    CloseHandle,
}

/// D3 动作结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    /// 动作类型。
    pub kind: ActionKind,
    /// 目标 ID（进程 / 线程 / 句柄）。
    pub target: String,
    /// 是否成功。
    pub ok: bool,
    /// 失败原因。
    pub error: Option<String>,
}

/// D4 内核模块 / 驱动。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelModuleInfo {
    /// 模块名。
    pub name: String,
    /// 路径。
    pub path: Option<String>,
    /// 基址（十六进制字符串）。
    pub base_address: Option<String>,
    /// 大小（字节）。
    pub size: Option<u64>,
}

/// D6 服务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// 服务名。
    pub name: String,
    /// 显示名。
    pub display_name: String,
    /// 运行状态：running / stopped / paused / unknown。
    pub state: String,
    /// 启动类型：auto / manual / disabled / boot / system / unknown。
    pub start_type: String,
    /// 运行账号。
    pub account: Option<String>,
    /// 可执行路径。
    pub binary_path: Option<String>,
    /// 是否内核驱动类服务（Windows）。
    pub is_driver: Option<bool>,
    /// 进程 ID（若在运行）。
    pub pid: Option<i32>,
}

/// 对外统一信封：一次调用的结果。
///
/// 成功时 `ok=true` 且 `data` 有值；失败时 `ok=false` 且 `error` 有值。
/// 平台不支持该能力时**必须**返回 `ok=false` 并给出原因，而不是返回空集合——
/// "不支持"与"支持但结果为空"是两件事，调用方需要能区分。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// 是否成功。
    pub ok: bool,
    /// 结果数据。
    pub data: Option<T>,
    /// 失败原因。
    pub error: Option<String>,
}

impl<T> Envelope<T> {
    /// 构造成功结果。
    pub fn ok(data: T) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    /// 构造失败结果。
    pub fn err<E: std::fmt::Display>(error: E) -> Self {
        Self { ok: false, data: None, error: Some(error.to_string()) }
    }
}

/// 不支持某能力时的标准错误消息，避免各平台各写一套措辞。
///
/// # 参数
/// * `what` - 能力名称
/// * `platform` - 当前平台标识
///
/// # 返回值
/// 统一的"不支持"描述
pub fn unsupported(what: &str, platform: &str) -> String {
    format!("平台 {} 不支持该能力: {}", platform, what)
}
