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
| **已声明但未实现（空壳）** | 原 **3 项**，现剩 **1 项**：签名验证（电池信息、Windows 句柄数值均已修）|
| **数值准确性** | 与任务管理器同源对照 **12 项全绿**（CI 日志实测「判定项 = 12，失败 = 0」）；生产验收累计修掉 **5 个**此前无人发现的真实数据缺陷（Windows 句柄少报 80%、首次 CPU 报 100%、Windows CPU 口径差 5pp、Linux 电池列表未排序、Windows CPU 使用 PDH 声明为无效的读数 +18.5pp）—— 详见 §八。这五项**都不崩不报错、返回结构不变**，代码审查与类型检查完全看不出来 |
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
| **1** | ~~**电池信息**（`BatteryInfo`）~~ **已实现（2026-10-01）** | 原为 `common.rs` 的 `batteries() { Vec::new() }` 空壳 | 三平台均已实现：Windows `GetSystemPowerStatus`、Linux `/sys/class/power_supply`、macOS `pmset -g batt`。新增 `battery.list` op，`system.snapshot.batteries` 由空壳转发到平台实现。**「无电池 -> 空列表」分支已在三平台验证**（run 36833755286：linux / darwin-arm64 各 33/33，windows 同批次的 prod_accept 33/33；三平台均为无电池设备：Windows 台式机 `BatteryFlag=128`、Linux 容器无 `/sys/class/power_supply`、macOS `pmset` 无电池行）。**取值分支在 Linux 上也已验证** —— 用 `mount --bind` 覆盖 `/sys/class/power_supply` 伪造可控电池条目（`tools/sysinformer-accept/battery_value_linux.py`，3 用例全绿，已接入 CI 的 Linux 腿）。**Windows / macOS 的真机取值仍需笔记本**，但已备好一条命令：`tools/sysinformer-accept/battery_verify_device.py` 会用操作系统**另一套独立视图**（WMI `Win32_Battery` + `powercfg /batteryreport`；`ioreg -rc AppleSmartBattery`）逐项对账 |
| **2** | **进程/模块签名验证**（`SignatureInfo`）| `model.rs:481` 声明结构，**全仓无任何构造点**；所有平台的 `signature:` 都是 `None`（`common.rs:458`、`platform_linux.rs:910/1051`、`platform_macos.rs:956`、`platform_windows.rs:274`）| `ProcessDetail.signature` 与 `ModuleInfo.signature` **永远返回 `null`**。调用方无法区分"没有签名"与"未实现签名验证" |
| **3** | **Windows 句柄的对象名与类型** | `platform_windows.rs` 的 `handles_of` 只填句柄值、访问掩码与 `ObjectTypeIndex`；`name`/`ref_count` 仍为 `None` | 能拿到句柄**编号**与**类型下标**（`type#N`），但看不出它指向哪个具体文件/注册表键。这是 System Informer"反向查找"功能（下方 §五.1）的前置 |

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

| # | 项 | 建议 | 状态 |
|---|---|---|---|
| 1 | `BatteryInfo` | 实现：Windows `GetSystemPowerStatus`、Linux `/sys/class/power_supply`、macOS `pmset -g batt` | ✅ **已完成 2026-10-01** |
| 2 | `SignatureInfo` | **实现**或**从模型移除**。实现：Windows `WinVerifyTrust`、macOS `SecStaticCodeCheckValidity`；Linux 无统一模型，可只标 `signed: null` | ⬜ 待决（**剩余唯一空壳**）|
| 3 | `EventKind::NetworkConnect` | **实现**或**从枚举移除**（让它走 `unsupported` 而不是静默不产出）| ⬜ 待决 |

### P1 —— 补实质能力差距

4. **Windows 句柄对象名/类型**（§二.3）— 需带超时的工作线程规避 `NtQueryObject` 挂死。
   数值侧已修（改用 `SystemExtendedHandleInformation`，类型下标已给），只差对象名
5. **Windows 栈帧符号名** — 引入 dbghelp
6. **反向查找 op**（§五.1）— 依赖 4

### P2 —— 补齐功能面

7. 关闭网络连接、IO 优先级、启动项/计划任务、服务增删改
8. Linux 的线程事件与 `ImageUnload`

### 不建议做

- **内存内容读取**：安全争议大（可被用于凭据窃取），且与"采集指标"的用途偏离

---

## 八、数值准确性实测（与任务管理器同源对照）

任务管理器的数字来自 **PDH 性能计数器**与 `GetPerformanceInfo`，因此这里用
**同一批数据源**与本库比对 —— 而不是与"肉眼看到的任务管理器"比，后者无法量化。
脚本 `tools/sysinformer-accept/taskmgr_compare.py`，Windows 实测 **12 项全绿**（CI 日志「判定项 = 12，失败 = 0」）
（`TASKMGR_COMPARE_OK`）**。

| 项 | 对照源 | 实测偏差 |
|---|---|---|
| 物理内存占用率 | `GetPerformanceInfo` | 0.02 pp |
| 可用 / 已用内存 | `GetPerformanceInfo` | 0.20% / 0.03% |
| 进程数 | `GetPerformanceInfo.ProcessCount` | 0.00% |
| 线程总数 | 逐进程 `process.threads` 求和 | 0.15% |
| **句柄总数** | 逐进程 `process.handles` 求和 | **0.91%** |
| 磁盘总量 / 可用（C/D/E 三卷） | `GetDiskFreeSpaceExW` | 0.00% |
| CPU 总占用率 | PDH `\Processor(_Total)\% Processor Time` | 均值差 +2.85 pp（16 组交替采样，正负各半）|

**结论：与任务管理器同源口径一致，无系统性偏差。** 逐次 CPU 差值可达 ±21pp，
来源是本机负载在 40%~97% 间剧烈波动 + 两侧采样窗口不完全重合；
正负各半、均值仅 +2.85pp 证明这不是算法偏差。

### CPU 口径偏差（第三轮修掉，此前一直在"解释"而不是"修"）

**根因（源码级）**：`sysinfo` 0.33.1 在 Windows 上**只注册 `% Idle Time`**，
用 `100.0 - idle` 推出使用率（其 `src/windows/system.rs`：
`add_english_counter(r"\Processor(_Total)\% Idle Time", ...)` 与
`set_cpu_usage(100.0 - total_idle_time)`）。而任务管理器用 **`% Processor Time`**，
两者分母不同：

| 计数器 | 分母 |
|---|---|
| `% Idle Time` | 全部时间（含 idle）|
| `% Processor Time` | **非 idle** 时间 |

有内核态活动（中断、DPC、系统调用）时二者必然不同。20 组采样实测
`sysinfo` 口径相对任务管理器**系统性偏高 +5.17pp**，95% CI `[+1.01, +9.33]` 不含 0。

**修法**：新增 `src/cpu_windows.rs`，Windows 直接读 PDH `% Processor Time`，
`common::cpu_all()` 优先用它的 `_Total` 与每核值，PDH 不可用时才回退 `sysinfo`。

### CPU 对照：为什么必须做窗口对齐

前几轮的"无系统性偏差"结论都不可靠，原因是**采样窗口量级不同**：

- 本库每次 refresh 的窗口只有 **27ms**（实测相邻调用时差 p50=27.2ms）
- PDH 的 `CookedValue` 窗口约 **1s**（实测 `Get-Counter` 单次耗时 1000~2840ms）

27ms 的窗口与 1s 的窗口，在负载于 40%~97% 剧烈波动的机器上，
逐次差标准差约 **14pp**，远大于待测量的几个 pp 偏差。四轮均值在
-2 ~ +4.3pp 间乱摆，无法定案。

**最终判据**（`tools/sysinformer-accept/cpu_windowed_compare.py`）：把本库
在 PDH 窗口内的多个样本取均值，使两侧被测区间拉平到同一量级。

| 口径 | 均值差 | 95% CI | 判定 |
|---|---|---|---|
| 旧（`100 - %Idle`） | **+2.957pp** | `[+1.310, +4.604]` 不含 0 | **FAILED** |
| 新（`% Processor Time`） | -1.607pp | `[-4.729, +1.514]` 含 0 | OK |
| 新（独立复跑） | -0.039pp | `[-2.142, +2.065]` 含 0 | OK |

**反向对照证明修复有效**：同一判据下旧口径稳定失败、新口径两轮通过。

### 生产验收找出的五个真实数据缺陷（此前完全无人发现）

这五项**编译、类型检查、生产验收的其余项、冒烟测试全部发现不了**，
只有拿独立数据源逐项对照、或让对照方与被测方分处不同实现时才暴露：

| # | 缺陷 | 影响 | 根因 |
|---|---|---|---|
| 1 | **Windows `process.handles` 系统性少报约 80%** | `OpenChamber.exe` 报 103 / 真实 514；`System` 报 1630 / 真实 7390；**47 个进程"有句柄却返回空"** | 用了已废弃的 `SystemHandleInformation`(类号 16)。Win10 2004+ 该类返回的记录**不再是** `SYSTEM_HANDLE_TABLE_ENTRY_INFO` —— 实测返回长度与记录数唯一吻合的 stride 是 **24 字节**（`8 + 150037×24 == 3600896`，精确匹配），而代码按 20 字节解析 → 偏移逐条错位 → pid 读错。已改用 `SystemExtendedHandleInformation`(类号 64，stride 40，pid 为 8 字节) |
| 2 | **首次调用 CPU 报 100%** | 进程内第一次调 `system.snapshot`，**12 核全部 `100.0%`**。任何新接入方第一次读到的都是错的 | `System::new()` 的上次累计时间为 0，首次 `refresh_cpu_all()` 把"开机至今"整段算成满载。已加 `refresh_cpu()` 预热：首次连刷两次、间隔 120ms。修复后首调为 `39.81%`（正常值）|
| 3 | **Windows CPU 口径与任务管理器不同** | 系统性偏高 +2.96 ~ +5.17pp（取决于判据） | `sysinfo` 只读 `% Idle Time` 并取 `100 - idle`，任务管理器用 `% Processor Time`，分母不同。详见上一节 |
| 4 | **Linux `battery.list` 未排序** | 多电池时返回顺序随文件系统而变，调用方不能依赖下标；采集端做前后快照比对会误报「电池变了」 | `std::fs::read_dir` 的顺序由文件系统决定（ext4 哈希序、tmpfs 插入序），Rust 明确不保证。实测两台机器给出**不同**顺序（Kali `USB AC BAT1 BAT0`、CI `BAT1 USB AC BAT0`），即 BAT1 排在 BAT0 前 |
| 5 | **Windows CPU 使用 PDH 声明为无效的读数** | CI 4 核 runner 上 `cpu.usage` **稳定在 ~20.2%**（sd 1.65pp）而真实值约 2%，与任务管理器差 **+18.5pp** | 采集间隔短于系统定时器节拍时 PDH 返回 `PDH_CALC_NEGATIVE_DENOMINATOR`(0x800007D6)，而代码只检查 API 返回码、**不检查 `val.CStatus`**，把 PDH 声明为无效的值当读数。runner 上该状态码占 **27.7%**。已修：检查 `CStatus` + 100ms 最小采集间隔，+18.5pp → **+1.0~2.5pp**。**注**：实测该状态下 PDH 写入的是 `0.0` 而非旧值，所以「为何稳定在 20%」的机制**尚未查明** |

第 4、5 项有一个共同特征值得单独记：**两者都不崩、不报错、不改变任何返回结构**，
只是让一个数字变成假的 —— #4 是「顺序不稳定」，#5 是「用了 PDH 声明为无效的值」。
这类缺陷只能靠**独立数据源对照**发现；代码审查与类型检查完全看不出来，
而任何「断言返回值合法」的冒烟测试都会通过。

修复 1 后实测：失败进程 49/316 → **4/317**，且这 4 个的真实句柄数确为 0
（`Registry` / `Secure System` / `Idle(pid=0)`，属正确行为）；
抽查 39 个进程条数一致率 **0/38 → 39/39**；
全系统句柄合计与 `GetPerformanceInfo.HandleCount` 偏差 **65.58% → 0.91%**。

代价：`process.handles` p50 57ms → 104ms（返回量增 5 倍）。
**这是正确数据的应有代价**，不是性能回退。

### 对照方法学的坑（与下节同源，此处补记）

| 坑 | 表现 | 教训 |
|---|---|---|
| 用 `GetSystemTimes` 当 CPU 基准 | 造出 **30+pp** 的假偏差，差点误判为"库有缺陷" | 它的 kernel 时间含 idle，与 PDH `% Processor Time` 口径不同。对照必须**同源** |
| api 与 ref 之间隔着"遍历 300+ 进程" | 内存差 6.28%、磁盘差 3.8% | 期间值一直在变。必须**紧邻采样**或多次取均值 |
| `Get-Counter` 逐核查 12 次 | 连 PDH 自身都不满足 `_Total ≈ 每核和/核数` | 每次调用是独立窗口，不在同一时刻，只能比"汇总 vs `_Total`" |
| 单轮 CPU 差值判 FAIL | 同条件两轮分别 2.08pp / 11.96pp | 必须交替采样 + 统计均值与符号分布 |
| `ctypes.wintypes.BYTE` 是有符号 | `BatteryFlag=128` 打成 `-128`，误判"本机有电池" | 显式用 `c_ubyte` |
| PDH ctypes 结构体 | 给 128 / 512 字节均 `PDH_INVALID_DATA` | `PDH_H_QUERY` 是变长不透明结构；改用 PowerShell `Get-Counter` |

---

## 九、审计方法学的坑（同一份代码三次矛盾结论）

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

## 十、附：本模块已完成的能力（对照，避免误读为"缺口"）

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