# utils-support-native-sysinformer

System Informer 能力的**跨平台可移植子集**，用于从 Java 高效采集本机指标。

System Informer（原名 Process Hacker）本身是 **Windows 独占**工具（README 写死
"Windows 10 or higher"）且带签名内核驱动 `KSystemInformer.sys`。本模块只做
**用户态可得的指标采集**，并把做不到的项逐条写明原因（见下方能力矩阵）。

---

## 一、模块结构

```
utils-support-native-sysinformer/           Rust cdylib + Java 25 Panama FFM 绑定
├── src/main/rust/
│   ├── Cargo.toml / build.sh / .cargo/config.toml
│   └── src/
│       ├── model.rs                全部数据结构（**公共契约，单人维护**）
│       ├── lib.rs                  C ABI + op 分发（只处理跨平台的 system.snapshot）
│       ├── common.rs               跨平台基线（sysinfo）
│       ├── platform_windows.rs     Windows 实现
│       ├── platform_linux.rs       Linux 实现
│       └── platform_macos.rs       macOS 实现
├── src/main/java/.../SysInformerNative.java     Java 25 FFM 绑定
└── src/main/resources/native/<平台>/            四平台产物

utils-support-native-sysinformer-java8/     Java 8 JNA 绑定（独立模块）
└── src/main/java/.../SysInformerJna.java
```

**为什么 Java 8 是独立模块**：`utils-support-common-starter` 的 `NativeLoader`
按 release 25 编译，JDK 8 引入后会在类加载阶段抛 `UnsupportedClassVersionError`。
所以 Java 8 侧自带动态库抽取逻辑，编译目标 release 8。

---

## 二、ABI

只暴露 **3 个 C 符号**：

| 符号 | 签名 | 说明 |
|---|---|---|
| `sysinformer_call` | `(const char* op, const char* args_json) -> char*` | 统一入口，按 op 名分发 |
| `sysinformer_free_string` | `(char* p)` | 释放返回值 |
| `sysinformer_version` | `() -> char*` | 库版本与构建目标 |

**新增能力不需要改 ABI**，Java 侧也不用重新绑定。

### 返回约定

一律返回 UTF-8 JSON 信封：

```json
{"ok": true,  "data": <结果>, "error": null}
{"ok": false, "data": null,   "error": "具体原因"}
```

**两种失败形态是刻意区分的，调用方必须都处理**：

| 形态 | 含义 | 例子 |
|---|---|---|
| `ok:false` | **能力不支持** | Windows 的内核态栈 |
| `ok:true` + `data.error` | **能力支持，但这一次调用失败** | Linux 读 `/proc/1/task/1/stack` 遇 `Permission denied` |

不支持的能力**绝不用空集合冒充**——"不支持"与"支持但为空"必须可区分。

---

## 三、op 一览表

### 系统级

| op | 参数 | 说明 | 权限 |
|---|---|---|---|
| `system.snapshot` | — | CPU 每核/汇总/负载、内存/swap、磁盘分区、网络接口、电池、时间线、主机信息，并并入平台相关项 | — |
| `disk.io` | — | 每磁盘读写字节/次数/队列深度 | Win 需管理员 |
| `kernel.modules` | — | 内核模块 / 驱动列表 | — |
| `service.list` | — | 服务列表（systemd / launchd / SCM） | — |
| `gpu.list` | — | GPU 适配器（厂商由 PCI VEN_ 判定） | — |
| `sensor.list` | — | 温度/风扇/电压 | 多半为空，见下 |
| `memory.modules` | — | 物理内存条（DDR4/2666MHz/Kingston…） | Win WMI / Linux DMI 需 root |
| `battery.list` | — | 电池列表（名称、百分比、状态、剩余/充满时间）。**无电池设备返回空列表**，不是错误 | — |

> **`battery.list` 此前漏在文档外**（2026-10-04 补）：它在三平台都有实现
> （`platform_windows.rs` / `platform_linux.rs` / `platform_macos.rs` 各有
> 分发点），验收里也一直在测（Linux 侧 `BATTERY_VALUE_OK` 3 用例），
> 但 README 的 op 表里没有它 —— 调用方无从得知这个 op 存在。
> 这是「代码有、文档无」的反向缺口，与 `NetworkConnect` 那种「文档有、
> 代码无」正好相反，但同样会让调用方误判能力边界。
>
> 取值来源：Windows `GetSystemPowerStatus`、Linux `/sys/class/power_supply`、
> macOS `pmset -g batt`。注意 **Windows/macOS 的真机取值尚未对账**
> （测试环境无电池设备），可用 `tools/sysinformer-accept/battery_verify_device.py`
> 在任意笔记本上验证；详见 `ACCEPTANCE.md` 未验项 #6。

### 进程级

| op | 参数 | 说明 | 权限 |
|---|---|---|---|
| `process.list` | — | 完整进程列表（**不截断**） | — |
| `process.tree` | — | 进程树（显式栈 + 环切断） | — |
| `process.detail` | `pid` | 单进程完整信息 | — |

`process.list` / `detail` 的字段：pid、ppid、名称、会话、用户、uid/gid、状态、优先级、
调度类、启动时刻、运行时长、WOW64、是否提升、是否受保护、CPU%、RSS、虚拟内存、
私有/共享字节、线程数、句柄数、**每进程 IO 读写字节与次数**、命令行与参数数组、
可执行路径、工作目录、根目录、`signature`。

> ⚠️ **`signature` 目前恒为 `null`。** `SignatureInfo` 在模型里已声明
> （`model.rs`），但**三平台都没有任何构造点**，签名验证属于**未实现**而非
> 原理性限制（Windows 可用 `WinVerifyTrust`、macOS 可用
> `SecStaticCodeCheckValidity`）。
>
> 这一点必须写明：调用方**无法**用「`signature == null`」判断「该进程没有
> 签名」—— 分不清「无签名」与「没查」。要判断签名有效性请走操作系统自己的
> 途径。详见 `GAP-ANALYSIS.md` 缺口 2。

### CPU 读数的窗口语义（影响「这个数准不准」）

`system.snapshot` 的 `cpu.usage` / `cpu.per_core` 是**窗口平均值**，
窗口长度由库内部的最小采集间隔决定，**2026-10-04 起为 1 秒**：

* 一秒内多次调用返回**同一个** 1s 平均值（不是每次都刷新）。
* 这个窗口是刻意选的：更短的窗口（曾用 100ms）会让读数在**低负载处虚高
  约 4~6pp** —— 短窗口里中断/DPC 与调用方自身采样开销的占比被放大，
  负载一高该占比回落，所以只在中低负载暴露，很难察觉。
  实测：空闲时偏差 **+4.14pp -> +0.08pp**（详见 `ACCEPTANCE.md` 未验项 #8）。
* 因此**不要**期望它像逐次瞬时值那样跳动；要瞬时值请直接用操作系统的
  计数器，但要接受同样的短窗口偏差。

判据与参照不依赖任务管理器：准确性的验证用**第一性原理**算
`(kernel+user-idle)/(kernel+user)`（Windows `GetSystemTimes` 累计值、
Linux `/proc/stat`），见 `tools/sysinformer-accept/cpu_accuracy.py`。

### 按需深度内省

**这些 op 不参与周期采样**——枚举全部进程的句柄会拖垮机器。

| op | 参数 | 说明 | 权限 |
|---|---|---|---|
| `process.threads` | `pid` | 线程列表（TID/优先级/CPU 时间/起始地址/栈基址/等待原因） | — |
| `process.env` | `pid` | 环境变量 | Win `PROCESS_VM_READ` / Linux 同 uid 或 root / macOS root |
| `process.handles` | `pid` | 句柄或 fd | Win 需管理员 / macOS 同 uid 或 root |
| `process.modules` | `pid` | 已加载模块（路径/基址/大小/版本） | macOS 仅本进程 |
| `process.credential` | `pid` | 凭据（见下） | 视平台 |
| `process.mappings` | `pid` | 内存映射 | macOS 仅本进程 |
| `process.stack` | `pid`,`tid`,`kernel` | 栈回溯 | 见下 |
| `socket.list` | `pid`? | TCP/UDP 连接（省略 pid 为全系统） | — |

### 控制动作

| op | 参数 | 支持的动作 |
|---|---|---|
| `action.exec` | `kind`,`target`,`arg` | `terminate` / `suspend` / `resume` / `set_priority` / `set_affinity` / `close_handle` |

**不提供代码注入与内核内存读写** —— 那是恶意软件技术，且不属于指标采集。

### 事件驱动

| op | 参数 | 说明 |
|---|---|---|
| `events.start` | `mask` | 订阅（位掩码见下），需管理员/root |
| `events.poll` | — | 取出已缓存事件（有界队列 4096，满则丢最旧） |
| `events.stop` | — | 取消订阅 |

事件类型（`EventKind`）与位掩码：

| 类型 | 位 | Windows(ETW) | Linux(netlink) | macOS |
|---|---|---|---|---|
| `ProcessStart` | 1 | ✅ | ✅ FORK | ❌ |
| `ProcessStop` | 2 | ✅ | ✅ EXIT | ❌ |
| `ThreadStart` | 4 | ✅ | — | ❌ |
| `ThreadStop` | 8 | ✅ | — | ❌ |
| `ImageLoad` | 16 | ✅ | ✅ EXEC | ❌ |
| `ImageUnload` | 32 | ✅ | — | ❌ |
| `NetworkConnect` | 64 | — | — | ❌ |

> ⚠️ **`NetworkConnect` 三平台都未实现**，属于「枚举里声明了、却永远不会有
> 事件产出」。按该类型过滤事件只会得到空结果，**不要**据此判断「没有网络
> 连接行为」。详见 `GAP-ANALYSIS.md` 缺口 3。

---

## 四、平台能力矩阵

> 「用户态」是本模块的硬边界。下面标 ❌ 的都是**原理上需要内核驱动或 Apple 授权**，
> 不是没做。

| 能力 | Windows | Linux | macOS |
|---|---|---|---|
| 进程列表 / 详情 / 树 | ✅ | ✅ | ✅ |
| 线程 | ✅ | ✅ | ✅ |
| 环境变量 | ✅ 读 PEB | ✅ `/proc/PID/environ` | ⚠️ 仅本 uid |
| 句柄 / fd | ✅ | ✅ fd | ✅ fd |
| 模块 | ✅ | ✅ | ⚠️ **仅本进程**（需 `task_for_pid`）|
| 凭据 | ✅ 令牌 SID/组/特权/完整性 | ⚠️ uid/gid/caps/seccomp | ⚠️ uid/gid |
| 内存映射 | ✅ | ✅ | ⚠️ **仅本进程** |
| 栈回溯 | ⚠️ **仅本进程**用户态 | ✅ **含内核栈**（需 root）| ❌ 需 `task_for_pid` |
| 套接字 | ✅ | ✅ | ✅ |
| 磁盘 IO | ✅ | ✅ | ✅ |
| 内核模块 | ✅ | ✅ | ✅ |
| 服务 | ✅ SCM | ✅ systemd | ✅ launchd |
| GPU | ✅ 仅适配器 | ✅ 使用率/显存 | ✅ ioreg |
| 传感器 | ⚠️ 取决于 BIOS | ✅ hwmon | ❌ AppleSMC 私有 |
| 内存条 | ✅ WMI | ✅ DMI（需 root）| ✅ system_profiler |
| 动作 | ✅ 5 种 | ✅ 5 种 | ⚠️ 4 种（无亲和性）|
| **事件驱动** | ✅ ETW | ✅ netlink | ❌ 需 entitlement |

### 各平台的边界与原因

**Windows**
- `process.stack` 只能回溯**调用线程自身**；其它进程需挂起线程 + `GetThreadContext` +
  `StackWalk64`（dbghelp），内核态栈需 ETW 栈事件 —— 均返回明确的不支持原因。
- 事件驱动走 ETW（`Microsoft-Windows-Kernel-Process`），**需管理员**，无需驱动。
- GPU 只给适配器名与厂商；使用率/显存需 `D3DKMTQueryStatistics`（结构含大联合体，
  偏移写错会内存损坏），相应字段为 `null` 而非 0。

**Linux**
- 最强的一档：`/proc` + `/sys` 覆盖几乎全部，**内核栈用户态可读**（`/proc/PID/stack`，需 root）。
- 事件驱动走 netlink proc connector（`CN_IDX_PROC`），需 root 或 `CAP_NET_ADMIN`。
- `memory.modules` 直接解析内核导出的 SMBIOS 表 `/sys/firmware/dmi/tables/DMI`，
  **不依赖 dmidecode**。

**macOS**（受 SIP 限制，三平台里最弱）
- 读**其它进程**的镜像/映射/栈需 `task_for_pid`，受 SIP 与代码签名限制 → 返回明确原因。
- **`events.*` 是硬限制**：系统级进程事件需 EndpointSecurity 框架及其 Apple 授权
  entitlement（`com.apple.developer.endpoint-security.client`），只签发给经 Apple
  批准的签名应用。代码里写明"**不以轮询伪装成事件**"。

---

## 五、构建与产物

```bash
# 四平台（CI 用）
./src/main/rust/build.sh windows x86_64   release
./src/main/rust/build.sh linux   x86_64   release
./src/main/rust/build.sh darwin  x86_64   release
./src/main/rust/build.sh darwin  aarch64  release
```

产物落 `src/main/resources/native/<平台>/`。

**本机（Windows）可用的本地编译路径**：

```bash
cargo +stable-x86_64-pc-windows-gnu build --release          # 真链接出 dll
cargo +stable-x86_64-pc-windows-gnu check --target x86_64-unknown-linux-gnu
cargo +stable-x86_64-pc-windows-gnu check --target x86_64-apple-darwin
cargo +stable-x86_64-pc-windows-gnu check --target aarch64-apple-darwin
```

> 默认 host 是 msvc，但本机没有 MSVC linker；装 `x86_64-pc-windows-gnu` toolchain
> 后可用 MinGW 的 gcc 链接。**交叉目标的 `check`（类型检查）也能本地跑**——
> 这使三平台代码都能在提交前验证，不必只靠 CI。

---

## 六、Java 侧用法

### Java 25（Panama FFM）

```java
import com.chua.nativesysinformer.support.SysInformerNative;
import com.fasterxml.jackson.databind.JsonNode;

JsonNode snap  = SysInformerNative.systemSnapshot();
JsonNode procs = SysInformerNative.processList();
JsonNode tree  = SysInformerNative.processTree();
JsonNode d     = SysInformerNative.processDetail(1234);

// 按需（昂贵）：线程 / 环境 / 句柄 / 模块 / 凭据 / 映射 / 栈
JsonNode threads = SysInformerNative.threads(1234);
JsonNode env     = SysInformerNative.envVars(1234);
JsonNode cred    = SysInformerNative.credential(1234);
JsonNode stk     = SysInformerNative.stackTrace(1234, 0, false);

// 事件驱动
SysInformerNative.startEvents(0xFFFF);
JsonNode events = SysInformerNative.pollEvents();
SysInformerNative.stopEvents();

// 需要区分失败形态时用原始信封
String raw = SysInformerNative.call("socket.list", "{}");
if (!SysInformerNative.isOk(raw)) {
    log.warn("socket.list 失败: {}", SysInformerNative.errorOf(raw));
}
```

### Java 8（JNA）

```java
import com.chua.nativesysinformer.java8.SysInformerJna;

String raw = SysInformerJna.call("process.list", "{}");
if (SysInformerJna.isOk(raw)) {
    // ... 用任意 JSON 库解析 raw
}
String why = SysInformerJna.errorOf(raw);
String ver = SysInformerJna.version();
```

> Java 8 侧**不引入 JSON 解析器**（保持零第三方 JSON 依赖），只提供
> `isOk` / `errorOf` 两个字符串级判断，解析交给调用方。

---

## 七、CI

`.github/workflows/native-sysinformer.yml`：四平台构建 + 导出符号断言 +
架构断言 + **运行时冒烟**。

运行时冒烟不是"符号存在"级别的检查，而是真加载并真调用：

| 腿 | 验了什么 |
|---|---|
| 全部 | `dlopen` → `version` 的 platform 与预期一致 → `system.snapshot` 含 cpu/host/memory → `process.list` 非空且字段齐全 → **未知 op 必须被拒绝** |
| Windows | 6 个平台专属 op 各断言"至少 N 条且字段非空" + `process.env` 与 `os.environ` **交叉核对** + `process.mappings` + **ETW 事件**（派生进程后必须收到 `ProcessStart`）+ 内核栈必须被显式拒绝 |
| Linux | **sudo 下 netlink 事件**（派生 `/bin/true` 后必须收到 `ProcessStart`）+ 非 root 对照（必须给出明确原因而不是崩掉）|
| 全部 | **JNA 冒烟**：`mvn package` → 用本腿刚编出的库 → `SysInformerJnaSmoke` |

---

## 八、设计取舍（读过代码再改）

1. **`model.rs` 是公共契约**，单人维护；三个平台文件各占一份、`#[cfg(target_os)]`
   门控，**每个只在自己平台参与编译** —— 这是并行开发不互相破坏的前提。
2. **`cpu_cores` 与 `cpu_summary` 必须走 `common::cpu_all()` 一次刷新**：分别调用会
   背靠背刷新两次，把 CPU 使用率的差值窗口压到微秒级（Windows PDH 会退化，恒为 100）。
3. **进程树用显式栈 + visited 集合**：恶意或异常的进程表可构造出环（A 的父是 B、
   B 的父是 A），递归会栈溢出并带走宿主 JVM。
4. **事件回调里绝不 panic**：panic 跨 FFI 边界展开进 C 代码属未定义行为。
   `lib.rs` 有 `catch_unwind` 兜底，但那是最后一道防线。
5. **零长指针必须先 `reinterpret`**：downcall 以 `ADDRESS` 返回的是 byteSize=0 的
   `MemorySegment`，直接 `getString(0)` 会抛 `No null terminator found`。
6. **`ValueLayout.JAVA_BYTE` 取回 `byte`，参与 int 运算会符号扩展**：
   真值 128/127 会算出 `|−128−127| = 255`，比较前必须 `& 0xFF`。
7. **Linux netlink 报文的三个易错点**（都实际踩过，且**编译发现不了**）：
   `nl_groups` 必须是 `1 << (CN_IDX_PROC-1)`；`nlmsg_type` 必须是 `NLMSG_DONE`(3)
   （用 0 = `NLMSG_NOOP` 会被内核丢弃）；`nlmsg_pid` 要填自身 PID。
   症状是"订阅成功却收不到任何事件"。
8. **Windows `SERVICE_STATUS_PROCESS` 的偏移**：`dwCurrentState` 在条目内 `+20`
   （不是 +16，那是 `dwServiceType`），`dwProcessId` 在 `+44`（不是 +48）。
   写错的症状是"297 个服务的状态全是 unknown"。
9. **`SERVICE_STATUS_PROCESS` / `SYSTEM_HANDLE_TABLE_ENTRY_INFO` 等结构按 x64 布局
   手工解析**，每处都带长度校验；长度字段不可信，一律设上限后扫描。
10. **`wmi` crate 只在 Windows 目标编译**。手写 COM vtable 在 Rust 里极易出错，
    且错误形态是内存损坏。

---

## 九、为什么某些 System Informer 功能不做

| 不做 | 原因 |
|---|---|
| 内核态栈回溯、内核回调、驱动级内存读写 | 需**签名内核驱动** `KSystemInformer.sys`，用户态拿不到 |
| 绕过 PPL / 强杀受保护进程 | 同上 |
| 卸载已加载 DLL / 解除文件占用 | 同上（且是动作而非指标） |
| 代码注入 | 恶意软件技术，非指标采集 |
| 在线查杀、自更新、用户标注 | 与指标无关，或属网络服务 |
| .NET 专属检查、窗口探查、硬件设备树 | Windows 独占，非跨平台指标 |
| macOS 事件驱动 | 需 Apple 授权 entitlement，见上 |