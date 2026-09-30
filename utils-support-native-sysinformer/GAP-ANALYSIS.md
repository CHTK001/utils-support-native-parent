# utils-support-native-sysinformer —— 功能缺口分析报告

**日期**：2026-09-30
**范围**：`utils-support-native-sysinformer`（Rust cdylib + Java 25 FFM / Java 8 JNA 双绑定）
**方法**：逐文件扫描三个平台实现 + `common.rs` + `lib.rs`，对每个 op 判定「实现 / 仅返回
不支持 / 缺」；对 `model.rs` 每个字段检查是否有**构造点**；再逐个核对「半实现」点。
**所有结论均附证据（文件:行号）**，不采用"标识符出现过就算实现"的判据 —— 该判据在本轮
审计中产生过多批假阳性（见末节）。

---

## 〇、结论摘要

| 维度 | 状态 |
|---|---|
| **op 层** | 21 个 op，**20 个三平台均已实现**；唯一缺口是 macOS 的 `events.*`（硬限制）|
| **已声明但未实现（空壳）** | **3 项**：电池信息、签名验证、Windows 句柄的对象名与类型 |
| **平台不对称** | Linux 最强；Windows 中等；macOS 受 SIP 限制最弱（已在 README 记录）|
| **事件类型** | 7 种中 **`NetworkConnect` 三平台都未实现**；Linux 缺 Thread\*/ImageUnload |
| **System Informer 有、本模块完全没有** | 8 类（反向查找、关闭连接、启动项、IO 优先级、内存读字节、进程创建、服务增删改、Windows 对象类型）|
| **明确不做（原理限制）** | 6 类（内核驱动相关 4 类 + macOS 事件 + 代码注入）|

**最该先修的是「空壳」那 3 项** —— 它们**声明了能力却永不产出**，比"没有这个功能"更容易
误导调用方（例如 `ProcessDetail.signature` 永远返回 `null`，调用方会以为是"这个进程没有签名"
而不是"这个功能没实现"）。

---

## 一、op 层覆盖（实测）

21 个 op 逐平台判定结果：

| op | Windows | Linux | macOS |
|---|---|---|---|
| `system.snapshot` | ✅ | ✅ | ✅ |
| `disk.io` | ✅ | ✅ | ✅ |
| `kernel.modules` | ✅ | ✅ | ✅ |
| `service.list` | ✅ | ✅ | ✅ |
| `gpu.list` | ✅ | ✅ | ✅ |
| `sensor.list` | ✅ | ✅ | ✅ |
| `memory.modules` | ✅ | ✅ | ✅ |
| `process.list` | ✅ | ✅ | ✅ |
| `process.tree` | ✅ | ✅ | ✅ |
| `process.detail` | ✅ | ✅ | ✅ |
| `process.threads` | ✅ | ✅ | ✅ |
| `process.env` | ✅ | ✅ | ✅ |
| `process.handles` | ✅ | ✅ | ✅ |
| `process.modules` | ✅ | ✅ | ✅ |
| `process.credential` | ✅ | ✅ | ✅ |
| `process.mappings` | ✅ | ✅ | ✅ |
| `process.stack` | ✅ | ✅ | ✅ |
| `socket.list` | ✅ | ✅ | ✅ |
| `action.exec` | ✅ | ✅ | ✅ |
| `events.start` / `poll` / `stop` | ✅ | ✅ | ❌ 硬限制 |

**说明**：`process.stack` 在 Windows 上只支持**本进程**（其它进程需挂起线程 +
`GetThreadContext` + `StackWalk64`），该分支明确返回不支持并说明原因，不算空壳。

---

## 二、已声明但**未实现**（空壳）—— 建议优先处理

| # | 项 | 证据 | 影响 |
|---|---|---|---|
| **1** | **电池信息**（`BatteryInfo`）| `common.rs:247` `pub fn batteries() -> Vec<BatteryInfo> { Vec::new() }` | 三平台都拿不到电量 / 充电状态 / 剩余时间。`BatteryInfo` 的 `percentage`、`time_to_empty_sec`、`time_to_full_sec` **从未被赋值** |
| **2** | **进程/模块签名验证**（`SignatureInfo`）| `model.rs:481` 声明结构，**全仓无任何构造点**；所有平台的 `signature:` 都是 `None`（`common.rs:458`、`platform_linux.rs:910/1051`、`platform_macos.rs:956`、`platform_windows.rs:274`）| `ProcessDetail.signature` 与 `ModuleInfo.signature` **永远返回 `null`**。调用方无法区分"没有签名"与"未实现签名验证" |
| **3** | **Windows 句柄的对象名与类型** | `platform_windows.rs` 的 `handles_of` 里写死 `kind: "unknown"`、`name: None`、`ref_count: None`，只填了句柄值与 access mask | 只能拿到句柄**编号**，看不出它指向什么（文件？事件？注册表键？）。句柄列表的价值主要在"这是什么对象"，此项缺失使其可用性大打折扣 |

### 关于第 3 项的技术说明

Windows 上要拿句柄的对象名需 `NtQueryObject(ObjectNameInformation)`，但它对**管道/同步
对象**等会**阻塞挂死**（这是 Windows 的已知行为，需靠"另一线程超时后放弃"来规避）。
因此该功能需要专门设计（带超时的工作线程），不是简单一行调用。
它是 System Informer 的"反向查找"功能（下方 §四.1）的前置。

---

## 三、平台能力不对称

| 能力 | Windows | Linux | macOS |
|---|---|---|---|
| 进程/线程/模块/映射/凭据 | 全 | 全 | 部分（读他人需 `task_for_pid`）|
| **栈回溯** | 仅本进程（无符号名）| **含内核栈**（需 root）| 不支持 |
| 环境变量 | 读 PEB（需 `PROCESS_VM_READ`）| `/proc/PID/environ` | 仅本 uid（root 可读他人）|
| 事件驱动 | ETW（需管理员）| netlink proc connector（需 root）| ❌ 硬限制 |
| 语义差异 | 有"令牌"概念 | 无令牌，映射为 uid/gid/capabilities | 无令牌，映射为 uid/gid |

**另一处不对称**：**Windows 栈帧的符号名为 `None`**（`platform_windows.rs:1375`），而
Linux 侧有（`platform_linux.rs:1931`，从 `/proc/PID/task/TID/stack` 解析出内核符号名）。
Windows 要符号名需引入 dbghelp（`SymInitialize` + `SymFromAddr`）。

---

## 四、`EventKind` 各类型的实际映射

`EventKind` 声明了 7 个变体，实际产出情况（只统计**真实映射点**）：

| 事件 | Windows(ETW) | Linux(netlink) | macOS |
|---|---|---|---|
| `ProcessStart` | ✅ | ✅ FORK | ❌ |
| `ProcessStop` | ✅ | ✅ EXIT | ❌ |
| `ThreadStart` | ✅ | ❌ | ❌ |
| `ThreadStop` | ✅ | ❌ | ❌ |
| `ImageLoad` | ✅ | ✅ EXEC | ❌ |
| `ImageUnload` | ✅ | ❌ | ❌ |
| **`NetworkConnect`** | **❌** | **❌** | ❌ |

**`NetworkConnect` 三平台均未实现 —— 属于"声明了却永不产出"**，与 §二 的 3 项同类。
Linux 的 `CN_IDX_PROC` 只提供 FORK/EXEC/EXIT，线程与 unload 事件需 `taskstats` 或
`perf_event` 通道。

---

## 五、System Informer 有、本模块**完全没有**的功能

| # | 功能 | 说明 | 难度 |
|---|---|---|---|
| 1 | **反向查找：谁占用了这个文件** | System Informer 的标志性功能。需遍历全系统句柄表并逐个取对象名 | 中（依赖 §二.3）|
| 2 | **关闭网络连接** | 断开某进程的连接。`action.exec` 目前只有 terminate / suspend / resume / set_priority / set_affinity / close_handle | 低-中 |
| 3 | **启动项 / 计划任务枚举** | 注册表 Run 键 / 计划任务 / systemd timer / launchd | 中 |
| 4 | **IO 优先级设置** | 有 CPU 优先级，无 IO 优先级 | 低 |
| 5 | **内存内容读取字节** | 只有 `process.mappings`（列出内存区域），**不能读内容** | 中（有安全争议）|
| 6 | **进程启动 / 创建** | 只有控制类动作，不能创建进程 | 中 |
| 7 | **服务增删改** | 只有枚举（`service.list`），不能创建/修改/删除服务 | 中 |
| 8 | **Windows 对象类型/名称** | 见 §二.3（属"半实现"）| 中 |

**已覆盖的对照项**（避免误列）：每进程网络连接 ✅（`socket.list` 支持按 pid 过滤）、
内核模块/驱动列表 ✅（`kernel.modules`）、令牌/凭据 ✅（`process.credential`）、
句柄枚举 ✅（`process.handles`，但缺名称）。

---

## 六、明确**不做**（原理上拿不到，非缺口）

| 项 | 原因 |
|---|---|
| 内核态栈回溯 / 内核回调 / 驱动级内存读写 | 需**签名内核驱动** `KSystemInformer.sys`（System Informer 自带此驱动，用户态拿不到）|
| 绕过 PPL / 强杀受保护进程 | 同上 |
| 卸载已加载 DLL / 解除文件占用 | 同上（且属"动作"非"指标"）|
| 代码注入 | 恶意软件技术，非指标采集 |
| **macOS `events.*`** | 需 EndpointSecurity 框架的 Apple 授权 entitlement（`com.apple.developer.endpoint-security.client`），只签发给经 Apple 批准的签名应用 |
| 在线查杀 / 自更新 / 用户标注 / 窗口探查 / .NET 专属检查 / 硬件设备树 | 与指标无关，或 Windows 独占 |

---

## 七、建议优先级

### P0 —— 修正「声明了却永不产出」（最易误导调用方）

| # | 项 | 建议 |
|---|---|---|
| 1 | `BatteryInfo` | **实现**（低难度：Windows `GetSystemPowerStatus`、Linux `/sys/class/power_supply`、macOS IOKit `IOPSCopyPowerSourcesInfo`）|
| 2 | `SignatureInfo` | **实现**或**从模型移除**。实现：Windows `WinVerifyTrust`、macOS `SecStaticCodeCheckValidity`；Linux 无统一模型，可只标 `signed: null` |
| 3 | `EventKind::NetworkConnect` | **实现**或**从枚举移除**（让它走 `unsupported` 而不是静默不产出）|

### P1 —— 补实质能力差距

4. **Windows 句柄对象名/类型**（§二.3）— 需带超时的工作线程规避 `NtQueryObject` 挂死
5. **Windows 栈帧符号名** — 引入 dbghelp
6. **反向查找 op**（§五.1）— 依赖 4

### P2 —— 补齐功能面

7. 关闭网络连接、IO 优先级、启动项/计划任务、服务增删改
8. Linux 的线程事件与 `ImageUnload`

### 不建议做

- **内存内容读取**：安全争议大（可被用于凭据窃取），且与"采集指标"的用途偏离

---

## 八、审计方法学的坑（同一份代码三次矛盾结论）

本报告的所有结论都不是第一版审计的结果。审计脚本连续给出三批**互相矛盾**的数字：

| 版本 | 报出的错误结论 | 错因 |
|---|---|---|
| 第 1 版 | 55 个字段"从未被赋值" | 搜索范围**漏了 `common.rs`** —— 那些字段全在那里被赋值 |
| 第 1 版 | `system.snapshot` 三平台都"缺" | 它由 `lib.rs` 统一处理，只扫了 platform 文件 |
| 第 1 版 | `NetworkConnect` 已映射 | 匹配到 `event_bit()` —— 该函数**列出全部 7 个变体**，不代表有产出 |
| 第 2 版 | `MemoryInfo.available` / `DiskPartition.available` "从未赋值" | 正则要求 `field:`，而**简写字段初始化**（`available,`）没有冒号 |
| 第 3 版（本报告）| 逐项回到原始代码核对 | — |

**教训**：判定"某功能有没有实现"必须看**赋值点 / 产出点**，不能看"标识符是否出现"
—— 类型声明、位掩码表、模型字段名都会让"出现即实现"失效。
本报告 §二 §三 §四 的每一项都已按证据列核对到具体文件与行号。

---

## 九、附：本模块已完成的能力（对照，避免误读为"缺口"）

- **op**：21 个（`system.snapshot` 含 CPU 每核/汇总/负载、内存/swap、磁盘、网络、主机、
  时间线、磁盘 IO、GPU；进程级 list/tree/detail；按需 threads/env/handles/modules/
  credential/mappings/stack/socket；动作 6 种；事件 3 个 op）
- **进程详情字段**：pid/ppid/名称/会话/用户/uid/gid/状态/优先级/调度类/启动时刻/运行时长/
  WOW64/提升/受保护/CPU%/RSS/虚拟内存/私有与共享字节/线程数/句柄数/**每进程 IO 读写字节与
  次数**/命令行与参数数组/可执行路径/工作目录/根目录
- **事件驱动**：三平台中 Windows(ETW) 与 Linux(netlink) **真实产出**内核事件（已真机验证）
- **双 Java 基线**：release 25（Panama FFM）+ release 8（JNA，独立模块）
- **性能**：`system.snapshot` p50 29ms、`process.list` 26ms（Windows）；Linux 侧 41ms
- **生产级验收**：Windows 30/30、Linux 真实机器 30/30（含并发/性能/泄漏/边界/数值对照）

验收详情见 `ACCEPTANCE.md`；本报告只关注**尚未实现**的部分。