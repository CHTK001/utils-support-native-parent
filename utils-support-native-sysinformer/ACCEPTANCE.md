# 验收清单（ACCEPTANCE）

本文件记录 **实际跑过的验证**，以及**没跑的部分及其原因**。目的是让接手的人不必
从头推断"这个模块到底验到什么程度"。

判定口径分两级：

| 口径 | 含义 | 本模块状态 |
|---|---|---|
| **能用** | 库能在目标平台加载、导出齐全、架构正确、绑定能编译、核心路径有功能断言 | ✅ 通过 |
| **生产级** | 并发 / 性能 / 资源泄漏 / 边界负例 / 数值对照，且四平台运行时均被覆盖 | ⚠️ **未完全覆盖**，见"未验项" |

---

## 一、验证方式与环境

| 平台 | 产物 | 验证方式 |
|---|---|---|
| windows-x86_64 | `sysinformer.dll` | 本机 Windows 真跑（ctypes 全项 + Java 25 FFM 冒烟 + 生产级验收 30/30）|
| linux-x86_64 | `libsysinformer.so` | **真实 Kali 机器**（Kernel 6.19 / x86_64），普通用户 + root 各一遍（生产级验收各 30/30）|
| darwin-aarch64 | `libsysinformer.dylib` | CI（macos-15，arm64 原生）真 dlopen + 冒烟 |
| darwin-x86_64 | `libsysinformer.dylib` | CI（**macos-15-intel**，Intel x86_64 原生）真 dlopen + 冒烟 |

**四平台全部有运行时验证。**

**重要**：所有验证都针对**仓库里入库的那份产物**，而不是本地重建的副本。
上传到 Kali 前做 md5 比对；Linux 入库产物的 md5 与在 Kali 上验证通过的那份一致。

---

## 二、CI 覆盖

`.github/workflows/native-sysinformer.yml`，四平台矩阵。每次运行都做：

| 步骤 | 验什么 |
|---|---|
| Build | 四平台原生构建 |
| Verify artifact exists | 产物确实生成（防止"构建成功但产物不存在"）|
| Check exports | 3 个 C 导出（`sysinformer_call` / `_free_string` / `_version`）整 token 精确匹配 + **负例对照**（不存在的名字必须查不到）|
| Check architecture | 直读容器头（PE/ELF/Mach-O），不认扩展名 |
| Runtime smoke (dlopen + version + system.snapshot) | 真加载 + 真调用；断言 platform 与预期一致、`system.snapshot` 含 cpu/host/memory、`process.list` 非空且字段齐全、**未知 op 必须被拒绝** |
| Runtime smoke (event driver, Linux netlink, needs root) | **sudo 下真收事件**：启动订阅 → 派生 `/bin/true` → **必须**收到 `ProcessStart`；另有非 root 对照（必须给明确原因而非崩溃）|
| Runtime smoke (Windows 专属 op) | 6 个平台专属 op 各断言"至少 N 条且字段非空" + `process.env` 与 `os.environ` **交叉核对** + `process.mappings` + **ETW 真收 ProcessStart** + 内核栈必须被显式拒绝 |
| Runtime smoke (JNA bridge, Java 8 module) | Java 8 侧 JNA 绑定端到端（先 `mvn package`，再用本腿刚编出的库）|
| Production acceptance | 四平台都跑，五维 30~33 项（见下）|
| **Battery value acceptance（仅 Linux）** | `mount --bind` 把可控目录覆盖到 `/sys/class/power_supply`，验证 `battery.list` 的**取值分支**（百分比、剩余时间换算、状态映射、非电池过滤、缺字段退化、**顺序契约**）。此前该分支在任何可用机器上都走不到，见未验项 #6 |
| Compare with Task Manager（仅 Windows）| 与任务管理器同源数据逐项对照；**非阻断**（CPU 口径判据的 +18pp 根因未定位，见未验项 #8），日志与判定标记照旧输出 |

`commit artifacts back`（`commit_back=true` 时）：把四平台产物回填到
`build/sysinformer-native-artifacts`，由维护者合并回 main。该步骤**显式透传退出码**，
推送失败会让 job 变红（不是被尾随的 `cat` 掩盖）。

---

## 三、生产级验收（`prod_accept.py`）

同一份脚本在 Windows 与 Kali 各跑一遍（口径一致），5 个维度：

### 1) 边界与负例（18 项）

非法 op / 空 op / 5000 字符超长 op / 畸形 JSON / 空参数串 / 缺 pid / pid 为负 /
pid 为 0 / pid 超大 / pid 为字符串 / pid 为浮点 / 未启动就 poll / 空 mask /
未知动作 / 动作目标非数字 / stack 缺 pid / stack 非法 tid / socket 非法 pid。

**要求**：全部返回**合法信封**（`ok:false` + 具体原因），**无一崩溃**。
FFI 里崩溃会带走宿主 JVM，这是生产事故。

### 2) 并发安全

8 线程 × 15 轮 × 6 op 并发混合调用；另有"事件订阅下 4 线程并发 poll"。

**要求**：无非法信封、无崩溃、结果一致。

### 3) 性能基线

每个 op 跑 30 次，报 p50 / p95 / max。用于容量规划（若按 1s 采样，单次开销即 p50）。

### 4) 资源泄漏

200 轮 × 4 op 后比对 RSS 与句柄/fd；事件启停 20 轮后比对句柄。

**要求**：不线性增长。

### 5) 数值对照

与系统工具交叉核对：进程数（`tasklist` / `ps -e`）、逻辑核数（`os.cpu_count()`）、
内存总量（`GlobalMemoryStatusEx` / `/proc/meminfo`）、自身 RSS（与 Python 侧比对）。

**要求**：差异在合理范围。这一步是"返回了结构但数字是错的"的唯一防线。

---

## 四、实测结果（最新入库产物）

### Windows（`sysinformer.dll` md5 `e43ee1b42542e02cc33f5c53a3bb3429`，850,944 B，run 37124428590，2026-10-03）

```
prod_accept.py        通过 33 / 失败 0   PROD_ACCEPT_OK
taskmgr_compare.py    判定项 12 / 失败 0  TASKMGR_COMPARE_OK
cpu_windowed_compare  有效配对 53（每对含 3~37 个本库样本，窗口中位 1002ms）
                      均值差 +2.051pp，容差 ±4.0pp -> TASKMGR_CPU_WINDOWED_OK

与任务管理器同源数据对照（GetPerformanceInfo，5 次紧邻采样取均值）：
  物理内存占用率   差 0.00pp        进程数        差  0.00%
  可用内存        差 0.00%         已用内存      差  0.00%
  线程总数        差 0.00%         句柄总数      差  0.15%
  磁盘 C:\ 总量/可用                全部 0.00%
  磁盘 D:\ 总量/可用                全部 0.00%

并发 8 线程 × 15 轮 × 6 op：120 次调用耗时 1.78s，错误 0 条
并发后连续两次 process.list 数量一致（134 vs 134）
事件订阅下 4 线程并发 poll：错误 0
泄漏：200 轮 × 4 op 后 RSS 29.0MB -> 29.7MB、句柄 424 -> 425
事件启停 20 轮（成功 20 次）：句柄 425 -> 425
```

> CPU 判据在同轮诊断里的原始数据（`cpu_rootcause_diag.py`，五源并置）：
>
> ```
> 有效配对 53（窗口中位 1002ms）  均值差 +2.051pp  容差 ±4.0pp -> OK
>
> [8] 同窗口配对比较（参考流间隔 100ms，与库的最小窗口等长）
>   A 本库        配对 2216  均值差 +1.343pp  sd 6.984  95% CI [+1.052, +1.634]
>   F 修复后逻辑   配对 2216  均值差 +2.525pp  sd 8.326  95% CI [+2.178, +2.872]
>   A − F 同迭代配对 2216      均值差 -1.182pp  sd 7.396  95% CI [-1.490, -0.874]
> ```
>
> **注意**：`cpu_rootcause_diag.py` 报告的「非 VALID 的 CStatus 占比」这一项
> 本轮日志未采集（旧脚本已随第 9 节一并更正），所以这里不列该数字 ——
> **宁可少写一项，也不引用无法在当前代码上复现的数字**。

### Linux（`libsysinformer.so` md5 `8502df90fe3a221f18fe11fc4dee3301`，1,569,176 B，run 37124428590，2026-10-03）

真实 Kali（`192.168.50.198`，普通用户 + root 各一遍，用仓库当前版
`prod_accept.py`，即含 3 条电池断言的那一版）：

```
普通用户  通过 33 / 失败 0   PROD_ACCEPT_OK
root      通过 33 / 失败 0   PROD_ACCEPT_OK
进程数对照：api=220 vs 系统工具=221（差异 <25%）
CPU 核数对照：api=8 vs os.cpu_count()=8
内存总量对照：api=16.95GB vs 系统=16.95GB（差 0.0%）
自身 RSS 对照：api=33.3MB vs python=33.3MB（差 0%）
并发 8 线程 × 15 轮 × 6 op：6.45s，无非法信封
泄漏：200 轮后 RSS +0.1MB、句柄 -1；事件启停 20 轮后句柄 +0
battery.list 与 system.snapshot.batteries 一致（0 vs 0 条，Kali 无电池）
```

**取值分支（Kali 上 bind mount 伪造 sysfs 实测，见未验项 #6）**：

```
battery_value_linux.py  case1 字段齐全 / case2 缺 power_now /
                        case3 多电池+状态别名+非电池过滤
                        KALI_BATTERY_VALUE_OK
  capacity=87                -> percentage=87.0
  status=Discharging         -> state=discharging
  energy_now/power_now=18/10 -> time_to_empty_sec=6480
  (energy_full-energy_now)/power_now=42/10 -> time_to_full_sec=15120
  缺 power_now               -> 退回内核 time_to_empty_now=5400，不编造
  Charging/Full             -> charging/full
  type=Mains / type=USB      -> 被过滤（不计入电池）
  无 energy_full             -> time_to_full_sec=null，不编造
  顺序                       -> ['B0','B1'] 升序（修复前是 ['B1','B0']）
```

守卫敏感性对照：同一夹具在**修复前**的产物上，case1/case2 全绿而 case3 报
`FAIL order: ['B1','B0'] sorted=False` —— 证明顺序断言不是恒真。

### macOS（run 37124428590，2026-10-03）

- `darwin-aarch64`：`libsysinformer.dylib` md5 `b6653cd845d7ce52b531015bf1cade9d`，1,119,664 B
- `darwin-x86_64`：`libsysinformer.dylib` md5 `f280d6d5b432514dabefb56147442fa5`，1,124,380 B

两腿均为 `SYSINFORMER_SMOKE_OK` + `SYSINFORMER_JNA_SMOKE_OK`
+ **31/31 `PROD_ACCEPT_OK`**（含电池断言，走 `pmset -g batt` 分支）。
泄漏检查（按平台缩放为 60 轮）：arm64 RSS 41.0MB→41.0MB 句柄 8→8；
x86_64 RSS 32.5MB→33.0MB 句柄 8→8。

### 四平台生产验收：全绿（2026-10-03，run 37124428590，sha `43de392`，结论 `success`）

> 这一轮编出的四份产物就是**当前入库的那一份**，且已被同 run 的
> attempt-2 独立复现（**四平台各 0 字节差异**），见第九节 5)。
>
> 注意这是**换了工具链与依赖锁定之后**的新基线，与 10-02 那批不可比 ——
> 那批产物的字节已被这次改动取代。

```
[OK] linux-x86_64     success
[OK] darwin-x86_64    success
[OK] darwin-aarch64   success
[OK] windows-x86_64   success
[OK] commit artifacts success
DONE  completed/success   RUN_OK
```

各平台的实测输出：

| 平台 | 运行时冒烟 | JNA(Java 8) | 生产验收 | 电池断言走的分支 |
|---|---|---|---|---|
| linux-x86_64 | `SYSINFORMER_SMOKE_OK` | `SYSINFORMER_JNA_SMOKE_OK` | **33/33** `PROD_ACCEPT_OK` | 无 `/sys/class/power_supply` -> 空列表 |
| windows-x86_64 | `SYSINFORMER_SMOKE_OK` | `SYSINFORMER_JNA_SMOKE_OK` | **33/33** `PROD_ACCEPT_OK` | `GetSystemPowerStatus` `BatteryFlag=128` -> 空列表 |
| darwin-aarch64 | `SYSINFORMER_SMOKE_OK` | `SYSINFORMER_JNA_SMOKE_OK` | **31/31** `PROD_ACCEPT_OK` | `pmset -g batt` 报 AC Power -> 空列表 |
| darwin-x86_64 | `SYSINFORMER_SMOKE_OK` | `SYSINFORMER_JNA_SMOKE_OK` | **31/31** `PROD_ACCEPT_OK` | `pmset -g batt` 报 AC Power -> 空列表 |

macOS 是 31 项而非 33：`events.*` 与事件订阅相关的 2 项按设计跳过
（需 EndpointSecurity 的 Apple 授权 entitlement，硬限制，已豁免）。
日志会如实写出「事件订阅不可用（平台 macos 不支持该能力: events.start）」，
不是静默跳过。

**windows 腿同时通过了任务管理器同源对照与 CPU 判定**：
`prod_accept` 33/33 -> `TASKMGR_COMPARE_OK`（12 项全绿）->
`TASKMGR_CPU_WINDOWED_OK`（均值差 +0.795pp ≤ 4.0pp）。
CI 每个平台都**重新构建**产物（`build.sh`），所以验的是含本轮全部改动的
**新代码**，不是仓库里的旧产物。

### darwin-x86_64 为何此前从未跑完

该腿要 **22~25 分钟**（缩放前），而 `concurrency: cancel-in-progress`
会让后续 push 把它取消掉 —— 连跑 4 次全是 `cancelled`，
等于**这个平台的生产验收从来没真正执行过**。

根因是 macOS 上 `process.list` / `process.detail` 单次很慢
（`sysinfo` 逐进程 `proc_pidinfo` 的固有成本，非本模块缺陷；
静态核查确认这两条路径上没有任何外部命令调用）。
**当时（2026-10-02，run 36995361316）实测**：arm64 约 1.8~2.2 秒、
x86_64 约 5.4~5.5 秒 —— 这一节是历史叙事，保留当时的数字以说明当时的判断依据。
**当前基线（run 37124428590）**：arm64 约 1.9~2.2 秒、x86_64 约 4.5~4.6 秒，
见上方「平台性能特征」表。
而验收脚本原本用与其他平台相同的采样次数 —— 泄漏检查一项在
x86_64 上就是 200 轮 × 约 5.5 秒 ≈ **18 分钟**，加上并发与性能段
整步远超 `concurrency: cancel-in-progress` 能容忍的窗口。

已按平台缩放（macOS 并发 6 / 性能 10 / 泄漏 60 / 事件 10，
断言条件与容差一行未改）。效果：aarch64 该步 251s，
x86_64 因 runner 本身较慢仍需约 14 分钟，但**能跑完了**
（整腿 36.2 分钟，`success`）。

### 平台性能特征（实测，影响采样周期选择）

同一份 op 在三平台上的 p50 差异极大，**根因是 `sysinfo` 的平台实现**
（Windows 走 `NtQuerySystemInformation` 一次全量、Linux 读 `/proc`、
macOS 逐进程 `proc_pidinfo`），不是本模块的封装开销：

| op | Windows | Linux | macOS arm64 | macOS x86_64 |
|---|---|---|---|---|
| `system.snapshot` | 4.27 ms | 1.90 ms | 48.48 ms | 57.40 ms |
| `process.list` | **6.01 ms** | 16.14 ms | **1891.24 ms** | **4462.90 ms** |
| `process.detail` | **0.93 ms** | 1.39 ms | **2150.32 ms** | **4590.43 ms** |
| `process.tree` | 4.14 ms | 3.53 ms | 1963.82 ms | 4436.89 ms |
| `process.threads` | 6.84 ms | 0.30 ms | 0.01 ms | 0.02 ms |
| `process.handles` | 4.18 ms | 4.90 ms | 0.01 ms | 0.04 ms |
| `process.mappings` | 1.96 ms | 1.32 ms | 0.38 ms | 0.96 ms |
| `process.modules` | 0.58 ms | 0.20 ms | 0.49 ms | 1.03 ms |
| `process.env` | 0.49 ms | 0.12 ms | 0.09 ms | 0.20 ms |
| `process.credential` | 1.76 ms | 0.07 ms | 8.08 ms | 16.55 ms |
| `kernel.modules` | 0.69 ms | 0.14 ms | 200.11 ms | 273.01 ms |
| `socket.list` | 0.33 ms | 11.99 ms | 19.52 ms | 25.12 ms |

四个平台**全部取自 run 37124428590 同一轮**，p50，同口径。
`process.list` 覆盖进程数：Windows 134、Linux 153、macOS arm64 496、
macOS x86_64 495 —— 绝对耗时随进程数走，比较时必须看这个量。

> ⚠️ **此前这张表只写了一个 macOS 数（约 2 秒），那是 arm64 的值。**
> 实测 Intel macOS 上 `process.list` 是 **4462.90 ms**、`process.detail`
> 是 **4590.43 ms**，比 arm64 慢约 **2.2 倍**。原来那句「单次约 2 秒」
> 对 Intel 用户是**低报了一倍有余**，据此定采样周期会直接翻车。

**对调用方的实际含义**：

- **Windows / Linux**：`process.list` 可按 1s 周期采样，开销可忽略
  （Windows 6.01ms、Linux 16.14ms）。
- **macOS arm64**：`process.list` / `process.detail` 单次约 **1.9~2.2 秒**。
- **macOS x86_64**：单次约 **4.5~4.6 秒**。
  按 1s 周期采样不只是「把 CPU 跑满」，而是**根本追不上** —— 每次调用
  自己就耗时 4.5 秒以上，永远处于上一轮还没结束的状态。
  macOS 上应改用 `system.snapshot`（arm64 48ms / x86_64 57ms）做高频指标，
  `process.list` / `detail` 只在需要时取，或放到 10s 量级的周期。

这一点也解释了 CI 上 macOS 腿为什么慢到跑不完：验收脚本原本用与其他
平台相同的采样次数（泄漏 200 轮 × 约 4 秒 = 13 分钟），整步要 22~25 分钟，
而 `concurrency: cancel-in-progress` 会让后续 push 把它取消掉 ——
`darwin-x86_64` 的生产验收因此连跑 4 次都是 `cancelled`。
现已按平台缩放采样次数（macOS 并发 6 / 性能 10 / 泄漏 60），
**断言条件、容差与信封校验未改**。

### 性能（Windows / Linux p50，同一轮 run 37124428590 实测）

| op | Windows p50 | Linux p50 | 说明 |
|---|---|---|---|
| system.snapshot | 4.27 ms | 1.90 ms | |
| process.list | 6.01 ms | 16.14 ms | |
| process.tree | 4.14 ms | 3.53 ms | |
| process.detail | 0.93 ms | 1.39 ms | |
| process.threads | 6.84 ms | 0.30 ms | |
| process.handles | 4.18 ms | 4.90 ms | |
| process.modules | 0.58 ms | 0.20 ms | |
| process.mappings | 1.96 ms | 1.32 ms | |
| process.env | 0.49 ms | 0.12 ms | |
| process.credential | 1.76 ms | 0.07 ms | |
| kernel.modules | 0.69 ms | 0.14 ms | |
| socket.list | 0.33 ms | 11.99 ms | |

（30 次采样，取 p50；两平台同 run、同口径，可直接横向比。
`process.list` 覆盖数：Windows 134 个进程、Linux 153 个进程 —— 绝对耗时
随进程数走，比较时要看这个量。macOS 两列见上方「平台性能特征」表。）

---

## 五、生产验收**发现并修复**的问题（全部是编译/类型检查发现不了的）

| # | 平台 | 问题 | 症状 | 根因 |
|---|---|---|---|---|
| 1 | Linux | netlink `nl_groups` 写错 | 订阅"成功"但 **0 事件** | `1<<1` 应为 `1<<(CN_IDX_PROC-1)`，订到了别的组 |
| 2 | Linux | netlink `nlmsg_type` 用 0 | 同上 | `NLMSG_NOOP` 被内核丢弃，须用 `NLMSG_DONE`(3) |
| 3 | Windows | `SERVICE_STATUS_PROCESS` 偏移错 | 297 个服务状态**全 unknown** | `dwCurrentState` 在 `+20` 不是 `+16`；`dwProcessId` 在 `+44` 不是 `+48` |
| 4 | Linux | DMI 结构解析 off-by-one | 内存条永远读不到 | 无字符串时下一结构在 `+2`（双 NUL），写成 `+1` → 提前终止、尾部整段漏掉 |
| 5 | Linux | `process.list` 缺 cmdline/cwd | 与 `process.detail` 不一致 | `fill_linux_fields` 依赖 sysinfo，而 sysinfo 的 `cmd()` 在此场景为空 |
| 6 | Linux | **sysinfo 把线程当进程** | 进程数虚高 6 倍（1307 vs 202）| sysinfo 递归 `/proc/<pid>/task/` 并 push 进同一列表；须用 `thread_kind() == Userland` 过滤 |
| 7 | Linux | **`battery.list` 未排序** | 多电池时返回顺序随文件系统而变（实测 `ls -U` = `USB AC BAT1 BAT0`，即 BAT1 排在 BAT0 前）| `std::fs::read_dir` 的返回顺序**由文件系统决定**（ext4 哈希序、tmpfs 插入序），Rust 明确不保证；重建目录或换文件系统后顺序即变。同文件其他列表（`services` 等）都已 `sort_by` 唯独它漏了 |
| — | Linux | 我自己的错误修复（中间版本）| 过滤没生效且更慢（683ms）| 用「顶层 `/proc/<pid>` 存在」当判据 —— **线程也有顶层目录**（`/proc/1041` 存在，`Tgid=686`）|

### 性能修复

| op | 优化前 p50 | 优化后 p50 | 原因 |
|---|---|---|---|
| system.snapshot | 565 ms | **29 ms** | 它不需要进程列表，却复用了"含全量进程"的刷新；另把 WMI 支撑的 `sensor.list`/`memory.modules` 移出快照（内存条是静态数据）|
| process.detail | 204 ms | **13 ms** | 原走全量枚举再筛一个，改为单进程刷新 |
| process.list | 158 ms | 26 ms | 刷新粒度拆分 |

事件启停成功率 2/20 → **11/20**（其余是 ETW 会话异步拆除的固有失败，已加有限重试并给明确原因）。

---

## 六、未验项（明确记录，不掩盖）

| # | 未验项 | 原因 | 影响 |
|---|---|---|---|
| 1 | ~~macOS x86_64 无运行时冒烟~~ **已关闭** | 改用 `macos-15-intel`（原生 Intel x86_64，Actions 最后一个 x86_64 镜像，支持到 2027-08）| run 36708484394 该腿 `Runtime smoke` 真实执行：dlopen x86_64 dylib 成功、`process.list 返回 502 个进程`、`SYSINFORMER_SMOKE_OK` |
| 2 | **Java 25 FFM 绑定：功能已验，CI 步骤已就位但被环境阻塞** | `SysInformerNative` 依赖 `utils-support-common-starter`（仅为 `NativeLoader`/`NativeUtils`）。**2026-10-03 实测更正**：该构件**不在** `packages.aliyun.com`（带凭据仍 404），真实来源是 GitHub Packages `maven.pkg.github.com/CHTK001/utils-support-resource-parent`。而 `com.chua` 下 420 个坐标里 **386 个 jar 的 `_remote.repositories` 仓库 id 为空**，即本机 `mvn install` 装入、从未发布；已发布的那份与本机**不是同一份字节**（sha1 `8d2d6db0…` vs `05fa2221…`）| 绑定的**正确性已验**：`mvn clean compile` exit 0（class major 69），且 `FfmSmoke` **端到端跑通 16 项断言**（真数据：279 个进程 / 25 个根）。`native-java-compile.yml` 的 paths 过滤已修（原写法只匹配到一个 README.md，门禁从未执行）、`FfmSmoke` 步骤已加入、失败分类已加（环境阻塞不弄红、真实编译错误仍变红）。**但 CI 侧仍无法真正编译** —— 凭据已配齐（aliyun ×2 + GitHub Packages ×2），卡在构件未发布。**未验项仍未关闭**，需独立决策是否 deploy 本地产物到远端 |
| 3 | **macOS `events.*`** | 系统级进程事件需 EndpointSecurity 框架及其 Apple 授权 entitlement（`com.apple.developer.endpoint-security.client`），只签发给经 Apple 批准的签名应用 | 硬限制。代码里明写"**不以轮询伪装成事件**" |
| 4 | **未做真实业务集成测试** | 属独立立项 | 本模块只保证"库本身可用且指标数值正确" |
| 5 | **未做长时间稳定性压测** | 属独立立项 | 目前只有 200 轮量级的泄漏检查 |
| 6 | **Linux 已关闭；Windows / macOS 需真硬件，但已备好一条命令** | 原以为测试环境无电池设备。Linux 侧改用 `mount --bind` 覆盖 `/sys/class/power_supply`，验证取值逻辑本身（3 用例全绿）。Windows 是台式机（`BatteryFlag=128`）、macOS CI 报 `AC Power`，这两个平台的**真机**对账没有设备跑不了 | **给常量映射写单元测试没有价值** —— 那只是复验读代码就能确认的东西；真正的未知是「`GetSystemPowerStatus` / `pmset` 在真笔记本上返回什么」，这没有硬件测不了。拿单元测试冒充「验过了」是自欺。因此改为交付**一条命令**：`tools/sysinformer-accept/battery_verify_device.py`，在任何 Windows 笔记本 / MacBook 上直接跑，用操作系统**另一套独立视图**（WMI `Win32_Battery` + `powercfg /batteryreport`；`ioreg -rc AppleSmartBattery`）逐项对账，对不上即真缺陷。无电池机器上会跑通「参考源采集通路」并明确输出 `BATTERY_DEVICE_SKIPPED_NO_BATTERY`（非静默跳过）|
| 7 | **CPU 判定的统计力有限** | 本机负载在 40%~97% 间剧烈波动，逐次差标准差约 14pp | 90s 采样得 85 个配对、标准误 1.2~1.6pp。判据是 **95% CI 含 0** 而非"逐点相等"——后者在这台机器上做不到。低负载或更长采样会显著收紧 |

| 8 | **CI 4 核 runner 上 CPU 判据未通过** —— **缺陷已修、偏差已消除；机制未完全查明** | 已确证：库会把 PDH 声明为无效的读数当成真值。`cpu_windows.rs` 只检查 API 返回码、**不检查 `val.CStatus`**；采集间隔短于系统定时器节拍时 PDH 返回 `PDH_CALC_NEGATIVE_DENOMINATOR`，该状态下 PDH 写入的 `doubleValue` **实测是 `0.0`**（不是旧值），而 runner 上这种读数占 **27.7%**（详见根因一节）。**但 `0.0` 只会把读数拉低，解释不了当时为何稳定在 ~20% —— 该机制未查明。** | 已修：检查 `CStatus`（正确性）+ 100ms 最小采集间隔（预防）。**实测 +18.5pp → +1.0~2.5pp**（两轮）。判据已改为按实测地板的容差判定并**恢复为阻断项**；有效性由 `--selftest` 用合成均值差保证 |

### CPU +18pp 根因（2026-10-02 定位并修复）

**症状**：CI 的 4 核 runner（Windows Server 2022 / AMD EPYC 7763 / 2 核 4 逻辑 /
Virtual Machine）上，本库 `cpu.usage` **稳定在 ~20.2%（标准差 1.65pp）**，
而 .NET 参考在 **0.00~4.83%** 剧烈波动，均值差 **+18.5pp**（CI `[+18.09, +18.98]`）。
本机 12 核同一份代码差 <3pp。

**反常之处不在偏差大小，而在形态**：真实 CPU 只有约 2%，参考源如实反映了
它的波动，而**库读数稳定得不像在测量**。

曾据此推断「库读到的是陈旧值」。**该推断已被实测推翻**，见下。

**根因**（本机实测复现，并测出阈值）：

`% Processor Time` 的原始采样时间戳按**系统定时器节拍**推进。若两次
`PdhCollectQueryData` 的间隔短于节拍，两次拿到同一时间戳，PDH 算出
**非正分母**，返回 `PDH_CALC_NEGATIVE_DENOMINATOR`（`0x800007D6`）。

**实测这个状态下 PDH 往 `doubleValue` 里写什么**（本机 12 核，8ms 采集 40s）：

```
CStatus 分布 = { VALID_DATA: 4051, CALC_NEGATIVE_DENOMINATOR: 304 }
无效读中 doubleValue 与「上一次有效值」逐位相同的比例 = 2/304 = 0.66%
典型样例： 0x800007d6  got=0.0   prev=100.0
```

即 **PDH 写的是 `0.0`，不是「保留上一次的值」**。本文档早期版本写的是
后者，与实测不符，已纠正。

而 `cpu_windows.rs` 原先只检查 `PdhGetFormattedCounterValue` 的 API 返回码，
**从不检查 `val.CStatus`**：

```rust
let st = PdhGetFormattedCounterValue(*c, PDH_FMT_DOUBLE, None, &mut val);
if st != PDH_SUCCESS { return None; }        // 只看 API 返回码
let d = val.Anonymous.doubleValue;           // CStatus 无效时实测是 0.0
if d.is_finite() { Some(d.clamp(0.0, 100.0) as f32) }   // 照样当真值用
```

验收脚本按 ~23ms 采样，**正落在失败区间内**。

**实测（2026-10-02，本机 12 核，8ms 采集 40s，n=304 次无效读）**：PDH 在
`CStatus = CALC_NEGATIVE_DENOMINATOR` 时把 `doubleValue` 写成 **`0.0`**，
**不是**「保留上一次的值」—— 逐位与上次有效值相同的只有 2/304 = 0.66%。
以下凡涉及「陈旧值」「冻结」的说法，均以此为准。

**因此因果链不完整，必须说清**：无效读是 `0.0`，只会把读数**拉低**，
不会拉高 18pp。所以「为什么当时是稳定在 ~20% 而不是被拉低」**尚未查明**。

已确证的是两件事：

1. 原代码确实会使用 PDH 明确声明为无效的读数 —— 这是缺陷本身，
   与那个值恰好是 0 还是别的无关；
2. 修复后 CI 上的偏差从 **+18.5pp 降到 +1.0~2.5pp**（实测两轮）。

缺陷成立、修复有效；但**机制未完全查明**。

**实测的阈值关系**（本机 12 核，`\Processor(_Total)\% Processor Time`）：

短窗口，每档 12s：

| 采集间隔 | 非 VALID 读数占比 | 状态码 |
|---|---|---|
| 5 ms | **26.18%** | `CALC_NEGATIVE_DENOMINATOR` |
| 10 ms | **17.22%** | 同上 |
| 15 ms | 1.68% | 同上 |
| 20 ms | **9.25%** | 同上 |
| 30 / 50 / 100 / 250 / 1000 ms | 0.00% | 全部 `VALID_DATA` |

**但长窗口（每档 40s）推翻了上表的 50ms**：

| 采集间隔 | 非 VALID 读数占比 |
|---|---|
| 30 ms | 0.23% |
| **50 ms** | **7.94%** ← 比 30ms 差得多 |
| 100 / 200 / 400 ms | 0.00% |

失败率**不是间隔的干净函数**：它取决于**当时的系统定时器节拍**，而 Windows
会做节拍合并（可到 31.25 / 62.5ms）。所以**不能**从「30ms 干净」推断
「50ms 也干净」—— 50ms 已被实测证伪。**100ms 是第一个有干净记录的周期**。

这是本轮最容易犯的错：我第一版修复取 50ms（当时只有 12s 短窗口数据），
被 40s 长窗口数据直接推翻。**阈值必须用长窗口测，且不能用单调性推断。**

**本机为什么测不出同等偏差**：本机多数轮次处于高负载（实测中位数 38%~100%）。
注意：既然无效读是 `0.0`，它只会拉低读数，所以「高负载把偏差掩盖了」这个
解释**本身也存疑** —— 本机测不出偏差的真实原因尚未查明。

**修复**（`cpu_windows.rs`）：

1. **检查 `CStatus`**（根本性）：`val.CStatus != PDH_CSTATUS_VALID_DATA`
   即视为无效，复用上一次**全部计数器都有效**的读数，绝不使用无效状态下的
   `doubleValue`。这一条保证**阈值取错也不会把陈旧值当读数**。
2. **强制最小采集间隔 100ms**（预防）：间隔不足时不采集，直接复用。
   代价是快于 10Hz 采样 CPU 时读到的是跨度 ≥100ms 的平均值；任务管理器
   默认刷新周期 1s、本模块建议周期也是 1s，实际几乎不触发。

两者是双保险：阈值错了只会让读数稍滞后，不会让读数变成假的。

### 修复效果与残余（CI 实测，run 36966468712 / 36907075264）

判据**恢复为阻断项后的首次运行**（run 36966468712）：

```
判定项 12 / 失败 0   TASKMGR_COMPARE_OK
均值差 = +2.519pp   标准差 1.60pp   95% CI [+2.097, +2.940]
**判定：均值差在容差内**（|+2.52| <= 4.0pp）
TASKMGR_CPU_WINDOWED_OK
```

**残余归属的决定性证据** —— 同一次运行里：

| 比较 | 均值差 | 说明 |
|---|---|---|
| **A 库 − F 同逻辑独立复刻**（同进程相邻采样） | **−0.129pp**，CI `[−0.513, +0.256]` **含 0** | 两个独立实现只差 **0.13pp** |
| A 库 − .NET（等长 100ms 窗口） | +1.820pp | |
| F 复刻 − .NET（同上） | +1.949pp | 与 A 几乎相同 |

即：**两个正确实现彼此差 0.13pp，而它们相对 .NET 都高约 1.9pp**。
那 1.9pp 是 `.NET PerformanceCounter` 的 cooked 值与裸 PDH 之间的属性，
与库的实现无关。

**判据均值会逐轮漂移**：+1.308pp（run 36907075264）→ +2.519pp（run 36966468712），
相差约 6 个标准误 —— 漂移来自 runner 实例与负载条件，不是测量误差。
这也是容差取 4.0pp（而非贴着 1.3pp 地板）的原因之一。

| 数据源 | 中位数 | 说明 |
|---|---|---|
| **A 本库**（修复后） | 5.26 | 被测对象 |
| **B 裸 PDH**（Python 独立复刻，高速率） | **22.37** | **修复前**的读法 |
| C .NET `PerformanceCounter`（100ms 窗口） | 2.02 | 参考 |
| D typeperf（2s 窗口） | 0.27 | 长窗口参考 |
| **F 修复后逻辑**（Python 独立复刻） | 3.99 | 与 A **同一套逻辑** |
| E WMI LoadPercentage | 1.00 | 粗粒度参考 |

runner 上 `CALC_NEGATIVE_DENOMINATOR` 占 **27.7%**，而修复后**复用兜底仅 0.5%**
（F 的行为分布：`collected 435 / reuse 1770 / stale-reuse 11`）。

**旧行为被逐位复现**：B 列（高速率裸 PDH）读 22.37，与修复前库的 20.2 吻合。

**CPU 判据：+18.5pp → +1.05pp**（等长 100ms 窗口下配对 2196 次，
`95% CI [+0.748, +1.348]`）。

### Java 25 FFM 绑定：本地端到端已验证，缺的是 CI 自动重复（2026-10-02 实测）

`src/smoke/java/FfmSmoke.java` 此前**从未在任何地方被执行过** ——
CI 只跑 Java 8 的 JNA 绑定。2026-10-02 当场补跑（本地 JDK 25 Corretto 25.0.3，
classpath 里的 dll 是 **CI 产出的那一份**，847,360 字节，与 artifact 一致）：

```
version = {"version":"0.1.0","platform":"windows","target":"x86_64"}
ASSERT ok   platform() 非空
ASSERT ok   system.snapshot 含 cpu/host/memory
ASSERT ok   process.list -> 279 个进程
ASSERT ok   process.tree -> 25 个根
ASSERT ok   process.detail(自身) name = java.exe
ASSERT ok   process.threads / handles / modules / credential 可调用
ASSERT ok   kernel.modules / service.list / socket.list 可调用
ASSERT ok   未知 op 被拒绝并取到原因
ASSERT ok   events.start 可调用 / events.poll 返回数组
JAVA25_FFM_SMOKE_OK     (exit 0，16 项断言全过)
```

**这条改变了未验项 #2 的性质**，必须说清区别：

| | 状态 |
|---|---|
| 绑定能编译 | ✅ 已验（`mvn clean compile` exit 0，class major 69）|
| 绑定能**正确运行** | ✅ **已验**（16 项断言，真数据：279 个进程 / 25 个根）|
| CI 能**自动重复**这一验证 | ⚠️ 步骤已就位，但**被环境阻塞**，见下 |

### 2026-10-03：阻塞原因从「缺凭据」更正为「构件未发布」

此前这里写的是「补法是一行：配 `MAVEN_ALIYUN_USER` / `MAVEN_ALIYUN_PASSWORD`」。
**实测证明那是错的**，已按事实更正：

| 仓库 | 匿名 | 带凭据 |
|---|---|---|
| aliyun release / snapshot | 401 | **404** |
| GitHub Packages（`github-resource`） | 401 | **200** |

**401 与 404 的区别是「凭据不对」与「找错仓库」的分界。** 该构件的真实来源是
GitHub Packages（`maven.pkg.github.com/CHTK001/utils-support-resource-parent`，
由 `utils-support-parent-starter-4.0.0.42.pom` 声明），与 aliyun 是不同主体。
已补配 GitHub Packages 的 secret，settings.xml 也已生成对应 server。

**但即便凭据齐全，Java 侧仍无法验证**，原因是环境：

```
扫描 D:\maven-repo 下 com\chua 的 420 个坐标（_remote.repositories）：
  jar 来源分布  (本地 install) 386 | aliyun release 6 | github-resource 1
```

**386 个 jar 的仓库 id 为空**，即由本机 `mvn install` 装入、从未发布到任何远端。
更关键的是唯一发布出去的那份 —— **同一 GAV 坐标，两份不同字节**：

```
utils-support-common-starter-4.0.0.42.jar
  本机 4,558,874 字节  sha1 8d2d6db08670189e60d7d6581f324d9677ae7c2b
  远端               sha1 05fa22216b0b96b1d908379cf680b7c3b63cde19
```

所以本机验证（FfmSmoke 跑通）用的是**本地那份**，CI 解析到的是**远端那份**，
不是同一个东西 —— **CI 全绿也不能证明本机验证的结论**。

### 已做的处置

`native-java-compile.yml`（run 37097495730 起生效）：

1. **paths 过滤修正**为 `utils-support-native-*/**`。原写法
   `utils-support-native-parent/**` 在本仓只匹配到一个 README.md，
   改真实模块**不触发** —— 该门禁历史上一次都没真正执行过。
2. `FfmSmoke` 步骤已加入，编译 + classpath + 运行 + 断言
   `JAVA25_FFM_SMOKE_OK`。
3. **失败分类**：全是 `com.chua` 依赖解析失败 -> warning + 摘要声明
   「Java 侧无法验证（环境阻塞）」且**不弄红**；出现其他失败 -> 变红。
   FfmSmoke 步骤同样分类，否则会与「已声明未验证」形成两个矛盾信号。

**一旦构件发布到远端，无需再改 workflow，FfmSmoke 会自动开始运行。**

### 2026-10-03：已在本地实跑验证该步骤本身可用

上面那句「无需再改 workflow」此前是**断言**，现已**实测**。在有全部构件的
本机（`D:\apache-maven-3.9.9-bin\apache-maven-3.9.9\conf\settings.xml`
把 `localRepository` 指向 `D:\maven-repo`，这正是 386 个构件的所在），
逐步跑了 CI 步骤体：

```
模块编译   mvn -f utils-support-native-sysinformer/pom.xml -DskipTests compile   exit 0
拼 classpath  dependency:build-classpath -Dmdep.outputFile=target/cp.txt       exit 0
编译冒烟   javac -encoding UTF-8 -cp <cp> -d target/smoke-ffm FfmSmoke.java      exit 0
运行       java --enable-native-access=ALL-UNNAMED -cp <cp> FfmSmoke            exit 0
           -> JAVA25_FFM_SMOKE_OK
```

即：**唯一缺的就是那些构件本身**，步骤体、classpath 拼接、native 加载、
断言与收尾标记都已验证可用。（本机 classpath 分隔符是 `;`，Linux 是 `:`；
CI 里已加 `tr ';' ':'` 兜底，不依赖插件「恰好写对了」。）

复现时踩到的两个坑，已写进 workflow 注释：

* **PowerShell 里 `-Dmdep.outputFile=x` 不加引号会被拆成 `-Dmdep` +
  `.outputFile=x`**，Maven 报 `Unknown lifecycle phase ".outputFile=x"`，
  看起来像插件没配好，实际是 shell 解析。CI 用 bash 不受影响。
* 本机本地仓库**不在** `~/.m2/repository`，而在 Maven 安装目录的
  `conf/settings.xml` 里指向 `D:\maven-repo`。换机器复现前要先确认这条，
  否则会误判成「本地也解析不到」。

未验项 #2 **仍未关闭**，但阻塞已从「缺凭据」更正为「构件未发布 / 同坐标不同字节」。
这是环境性阻塞，需独立决策是否把本机 `mvn install` 的产物 deploy 到远端。

同一模块的 Java 8 侧（`SysInformerJnaSmoke`）**已在 CI 里真跑**，那条腿是自动的。

---

逐项排除：

| 候选原因 | 排除依据 |
|---|---|
| 仍有无效读被采用 | F 的 `stale-reuse` 仅 **11/2216 = 0.5%** |
| 测量装置自扰动 | 本进程占系统 CPU **0.67%**（4 核机上 0.03 个核）|
| 汇总与每核口径不一致 | 「`cpu.usage` − 每核均值」平均 **-0.26pp** |
| 参考源自身不可信 | `D typeperf − C .NET = -0.84pp`，两参考源同源一致 |
| **Rust 实现的特有偏差** | **不成立**：F 是**同一套逻辑**的 Python 独立复刻，偏差 **+2.14pp**，比真库 **+1.05pp 还大** |

最后一条是决定性的：**偏差在独立实现里更大**，说明它不是 Rust 代码的性质，
而是「两个独立采样的进程，读同一计数器，在突发负载的 4 核 VM 上互相比对」
这件事本身的精度限制。

具体机制：库侧窗口 100~120ms（100ms 下限 + 20ms 调用周期量化），
.NET 侧 `NextValue()` 的有效窗口取决于它自己的循环耗时；两者不是同一批采样时刻，
配对容差 ±60ms。在 sd 7.18pp 的信号上，几十毫秒的错配就能造出约 1pp 的系统性差。

### 判据本身的修正（2026-10-02）

原判据是「95% CI 含 0」。实测**原理上不可达**：

* 本库按设计报告「最旧 100ms 的窗口平均值」（`MIN_COLLECT_INTERVAL_MS`），
  而 .NET 参考的窗口由它自己的循环耗时决定 —— **两者窗口终点不同**。
  低负载且突发的工作负载上标准差达 7~19pp。
* 两侧是两个独立进程，配对容差 ±60ms；几十毫秒错配就造出约 1pp。
* **实测「实现差」**：把**同一套逻辑**用 Python 独立复刻，在**同一进程、相邻
  采样**下与真库比较。

  | 测量地点 | A − F | 95% CI | 条件 |
  |---|---|---|---|
  | **CI runner（干净）** | **−0.129pp** | `[−0.513, +0.256]` 含 0 | 负载低且平稳，n=2115 |
  | 本机（脏） | −1.273pp | `[−2.528, −0.018]` | 负载剧烈波动，sd 18.6pp |

  **以 CI 的 −0.129pp 为准**：同一逻辑的两个独立实现只差 0.13pp。
  本机那个 −1.273pp 是**在波动负载下测的**，标准差 18.6pp，量级不可信 ——
  我早期把它当作「地板」写进了代码注释与本文档，那是**取错了测量**。

  真正需要容差覆盖的不是「实现差」，而是**相对 .NET 的系统性偏移**
  （见下）+ **逐轮漂移**。

改为「均值差在容差内」：

| 项 | 值 | 依据 |
|---|---|---|
| 地板 | ≈1.3pp | 实测，同逻辑独立实现，单次点估计 |
| 容差 `TOL_PP` | **4.0pp** | 3 倍地板 |
| 目标缺陷 | +18.5pp | 修复前的偏差，= **4.6 倍**容差 |

取 3 倍而非贴着地板：地板是单次点估计，而均值在波动负载下也会飘
（实测本机 sd 9.89pp、43 配对时均值到过 −2.70pp）。**随机红绿的门等于
没有门** —— 被习惯性忽略之后连抓缺陷的作用也没了。

**这个改动明确降低了判据强度**，记在这里以免被误读成「原判据是错的」：
失去的只是「分辨 1pp 以下偏差」的能力，而那种精度本来就不可达。

判据有效性由 `cpu_windowed_compare.py --selftest` 用**合成均值差**保证
（不依赖被测机器）：验证它仍能抓住 ±18.5pp、放过 ±3.9pp、
并断言「目标缺陷 / 容差 ≥ 3 倍」。

**该判据自 2026-10-02 起恢复为 CI 阻断项。**

### 本机（12 核）为什么测不出来

本机多数轮次处于高负载（实测中位数 38%~100%），等长窗口配对
在本机得 `+0.814pp`，`95% CI [-0.518, +2.147]` **含 0** —— 即满载时两侧本来就一致。
只有 CI 那种**低负载且突发**的环境才会把问题放大。

**已并排除的对照实验**（避免把「测不出来」当成「不存在」）：

| 实验 | 结果 | 说明 |
|---|---|---|
| 造已知真值（k 个忙等进程 → k/核数×100%） | **无效** | 本机已满载，起 1 个进程时五个源全读 ~100%（真值 8.33%），新增负载挤不进已饱和的机器 |
| 窗口对齐对照（库也按 1000ms 采样，与 .NET 同窗口配对） | **+0.031pp，CI [-2.59, +2.65]，无系统性偏差**；84 个配对窗口里四元组两列差**逐行相等** | 证明库与 .NET 测的是同一个量、口径没问题。**排除了「口径差」** |
| 现有 `cpu_windowed_compare.py`（库 23ms vs 参考 1s，窗口内平均） | 本机 -1.281pp，CI [-3.51, +0.95]，无系统性偏差 | 说明「窗口内平均」这个方法本身成立（相邻 PDH 窗口首尾相接铺满时间轴） |

---

### 定位过程中逐项排除的假设（2026-10-01 ~ 10-02，共十一项）

> 这些不是「顺带排除」的边角料：#10 与 #7 都是**我自己的假设被自己的实验推翻**，
> 记下来是为了避免下次再犯同样的错。

| # | 假设 | 排除依据 |
|---|---|---|
| 1 | 参考源选错 | 已改用 PDH `\Processor(_Total)\% Processor Time`（任务管理器同源） |
| 2 | 核数不匹配 / `.NET ProcessorCount` 归一化 | CI 上 `.NET=4` `PDH=4` `WMI=4`，比值 **1.0000** |
| 3 | PDH 实例子集取错 | 逐项对照过每实例值 |
| 4 | 库内口径混用（部分核回退 sysinfo） | CI 日志「`cpu.usage` − 每核均值」平均 **−0.43pp**、超 0.01pp 占比 5%。若走 sysinfo 回退，该差值由构造保证**恒为 0**（`usage` 就是每核均值），故 CI 走的是 PDH 路径 |
| 5 | 参考源自身不自洽 | 已用 `_Total` vs 同条实例均值交叉核对（CI 最大 1.241pp，**此项反而支持「参考侧有问题」**）|
| 6 | PDH 流首采样值不可靠 | 已改为长驻查询 + `NextValue()` 预热 |
| 7 | 逐核读数退化（per-instance 读取失效）| 我曾据「CI 四核读数完全相同」立此假设，**被自己的实验推翻**：本机原始浮点值显示 PDH 在短窗口下本就量化重复（`91.168022` 出现 3 次且位模式完全相同）。该假设作废，判据已删除 |
| 8 | 采样窗口长度（库 ~23ms vs 参考 ~1s）| 本机扫描 P ∈ {20ms, 50ms, 100ms, 250ms, 500ms, 1s, 2s}，库与参考的差在各档均 <3pp，不随时长单调变化 |
| 9 | 库的 PDH 路径本身不忠实 | 本机用 Python **独立复刻** `cpu_windows.rs` 的裸 PDH 查询作对照：`A 本库 − B 裸PDH = −0.44pp / 0.00pp`（两次实测），即库读到的就是裸 PDH 的值 |
| 10 | 采集失败 / `CStatus` 无效被当有效 | **这一项最终被证实是真因**（见上方根因一节）。当时本机 `PdhCollectQueryData` 0/101 失败、`CStatus` 表面全 VALID，是因为我只统计了**采集**的返回码，没看 `CStatus`；实际 `CStatus` 里混着 2.3%~3.2% 的 `CALC_NEGATIVE_DENOMINATOR`（runner 上更高达 27.7%）。**盲区来自只查了一半的状态** |
| 11 | 残余 ~1pp 也是同类问题 | 否。同逻辑的 Python 复刻偏差 **+2.14pp > 真库 +1.05pp**，说明残余是「两个独立进程比对」的地板，与实现无关（见上节） |

**本机为何无法复现**：造「真值 = k/核数 × 100%」的已知负载实验失败了 ——
本机已被其它会话占满，起 1 个忙等进程时五个数据源全部读 ~100%（真值 8.33%），
即新增负载挤不进已饱和的机器。因此**只能到出问题的 runner 上量**。

**下一步**：新增 CI 步骤 `CPU root-cause diag`（Windows only，非阻断），
用 `tools/sysinformer-accept/cpu_rootcause_diag.py` 在 runner 上并置五个源：

```
A 本库     system.snapshot 的 cpu.usage
B 裸 PDH   Python 独立查询（复刻 cpu_windows.rs），并报告 collect 返回码与 CStatus
C .NET     PerformanceCounter（既有参考流）
D typeperf Windows 自带 CLI，另一条 PDH 代码路径
E WMI      Win32_Processor LoadPercentage，完全不同的栈
```

判读表：`C ≈ D` 说明 .NET 与 typeperf 同源一致；若 `A ≈ B ≈ D` 而 `C/E` 都低，
则问题在 `PdhGetFormattedCounterValue` 瞬时值与 cooked 值的口径差异。

---

## 七、2026-10-01 轮：数值准确性实测发现并修复的三个缺陷

这一轮的起因是回答"数据准不准、比不比任务管理器准"。方法是**不用肉眼比对**，
而是拿任务管理器的**同源数据源**（PDH 性能计数器与 `GetPerformanceInfo`）逐项比。

工具（`tools/sysysinformer-accept/`）：

| 脚本 | 作用 | 判定标记 |
|---|---|---|
| `taskmgr_compare.py` | 内存/进程/线程/句柄/磁盘/电池 与任务管理器同源数据对照 | `TASKMGR_COMPARE_OK` |
| `cpu_windowed_compare.py` | **CPU 口径判定**（窗口对齐，权威）| `TASKMGR_CPU_WINDOWED_OK` |
| `cpu_aligned_compare.py` | CPU 瞬时配对版本（参考，判定不用它）| `TASKMGR_CPU_ALIGNED_OK` |
| `pdh_stream.ps1` | 供上面两个脚本使用的 PDH 时间戳流 | — |

### 修掉的三个缺陷

| # | 缺陷 | 影响 | 根因 |
|---|---|---|---|
| 1 | **Windows `process.handles` 少报约 80%** | `OpenChamber.exe` 报 103 / 真实 514；`System` 报 1630 / 真实 7390；**47 个进程"明明有句柄却返回空"** | 用了已废弃的 `SystemHandleInformation`(类号 16)。Win10 2004+ 该类返回的记录**不再是** `SYSTEM_HANDLE_TABLE_ENTRY_INFO` —— 实测返回长度与记录数唯一吻合的 stride 是 **24 字节**（`8 + 150037×24 == 3600896`，精确匹配），代码按 20 字节解析 → 偏移逐条错位 → pid 读错。已改用 `SystemExtendedHandleInformation`(类号 64，stride 40) |
| 2 | **首次调用 CPU 报 100%** | 进程内第一次调 `system.snapshot`，**12 核全部 `100.0%`** | `System::new()` 上次累计为 0，首次 `refresh_cpu_all()` 把"开机至今"整段算成满载。已加 `refresh_cpu()` 预热（连刷两次、间隔 120ms）|
| 3 | **Windows CPU 口径与任务管理器不同** | 系统性偏高 +2.96 ~ +5.17pp | `sysinfo` 0.33.1 在 Windows 上只注册 `% Idle Time` 并用 `100.0 - idle`（其 `src/windows/system.rs` 可查），而任务管理器用 `% Processor Time`。两者分母不同：前者分母含 idle，后者分母是非 idle 时间。已新增 `cpu_windows.rs` 直读 PDH `% Processor Time` |

**这三个缺陷，`cargo check`、类型检查、30 项生产验收、冒烟测试全部发现不了。**

### 修复效果（实测，全部打在入库产物上）

| 指标 | 修复前 | 修复后 |
|---|---|---|
| 句柄失败进程 | 49 / 316 | **4 / 317**（这 4 个真实句柄数确为 0：`Registry`/`Secure System`/`Idle(pid=0)`）|
| 有句柄却返回空 | **47** | **0** |
| 抽查条数一致率 | 0 / 38 | **39 / 39** |
| 句柄总数 vs `GetPerformanceInfo.HandleCount` | 偏差 65.58% | **偏差 0.91%**（复跑 0.06%）|
| 首次 CPU | 12 核全 `100.0%` | `39.81%`（正常值）|
| CPU 口径 | +2.957pp，CI 不含 0（FAILED）| 两轮独立：-1.607pp / -0.039pp，CI 均含 0（OK）|

**CPU 修复的反向对照**：把口径切回旧实现（只读 `% Idle Time`）后重编译，
同一判据下 `+2.957pp`、95% CI `[+1.310, +4.604]` 不含 0 → **FAILED**。
这证明判据本身能抓住该缺陷，不是"怎么测都过"。

代价：`process.handles` p50 从 57ms 升到 104ms（返回量增 5 倍）。
**这是正确数据的应有代价，不是性能回退。**

### CPU 判据在 CI 上失败的三轮定位（对照源的问题，不是库）

windows runner 上判据连续报 +18.965pp / +17.973pp / +16.313pp，
标准差只有 2pp 上下（很稳定，不是噪声）。逐轮排除：

**第 1 轮：怀疑参考源选错。** 原判据用 `sum(每核)/核数` 作参考。
本机实测证明这不是原因，但顺带确认了一件事：
```
本库 cpu.usage 与每核均值的最大差 = 0.0000pp（12 核，占比 0%）
PDH  _Total 与全部实例均值的差   = -0.000pp
```
即库内部无口径混用、PDH 侧自洽。参考源已改为 `cpu.usage`（= `_Total`），
但 CI 上仍报 +17.973pp —— **根因不在参考源**。

**第 2 轮：加三类诊断，排除核数与子集问题。**（4 核 runner 实测）
```
GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) = 4
PDH \Processor(*) 实际返回的实例 = 4 个 -> ['0','1','2','3']
本库 cpu_cores 数量 = 4   cpu.logical_count = 4
PDH 自洽性（_Total vs 同条实例均值）: 最大 0.569pp  -> ref 可信
```
核数三项全部一致，PDH 侧也自洽（0.569pp 远小于 16pp）。

**第 3 轮：四元组对照，决定性。**
```
本库_Total | 本库每核均值 | PDH_Total | PDH实例均值
    95.82  |      95.82    |   79.72   |    79.72
[本库_T-PDH_T = +16.10    本库核-PDH核 = +16.10]
```
**两列差完全相等**。既然本库内部自洽、PDH 也自洽，而整体差同一个值，
那问题就不在任一计数器，而在**两边测的不是同一段时间**。

根因：`pdh_stream.ps1` 每输出一个值就调一次 `Get-Counter`，
即**每轮都新开一个查询**。PDH 的
`CookedValue = (raw_now - raw_first) / (t_now - t_first)`
里 `raw_first/t_first` 取自该查询自己的上一个采样点；新开查询时基线未稳定，
于是**每个输出值都是「首采样值」**，系统性偏低，在 4 核 runner 上表现为
本库恒高约 16pp。

修法：计数器只建一次 → 每个先 `NextValue()` 建立基线 → `sleep` 后取值 →
时间戳打在**读取时刻**。本机效果：

| 指标 | 修前 | 修后 |
|---|---|---|
| 标准差 | 8.96pp | **6.56pp** |
| 四元组平均绝对差 | 6.34pp | **3.25pp** |
| PDH 自洽性最大差 | — | 0.287pp |
| 判定 | OK | OK |

顺带一个坑：`PerformanceCounter` 构造器签名是
`(categoryName, counterName, instanceName)`；传成
`(Processor, _Total, %ProcessorTime)` 会报
`Could not locate Performance Counter`，这个错也踩过一次。

**教训**：对照脚本本身和被测代码一样会错，而且错得更有欺骗性 ——
它不会崩、不会明显报错，只会让结论偏一个稳定的常数。
本轮三次都是靠"加诊断看真实数字"推进的，前两次都曾差点归错因。

### CPU 判定为什么必须做窗口对齐

这是本轮最费时也最要紧的一点：

- 本库每次 refresh 的窗口只有 **27ms**（实测相邻调用时差 p50=27.2ms）
- PDH 的 `CookedValue` 窗口约 **1s**（实测 `Get-Counter` 单次耗时 1000~2840ms）

两侧量级差 50 倍时，逐次差标准差约 **14pp**，远大于待测量的几个 pp。
开发中四轮均值在 **-2 ~ +4.3pp** 间乱摆，一度误判为"已修好"。
最终判据改为「把本库在 PDH 窗口内的多样本取均值」，把两侧被测区间拉平。

对照方法自身的坑（完整清单见 `tools/sysinformer-accept/README.md`）：
用自己墙钟给 PDH 样本打时间戳会整体偏移 1~2.8s（必须用计数器自报的
`Timestamp`）；分别 `Get-Counter` 取两个计数器再比较无效（窗口不同）；
`GetSystemTimes` 不能当 CPU 基准（其 kernel 含 idle，会造出 30+pp 假偏差）。

---

## 八、可复现的验证命令

```bash
# 1) 生产级验收（边界/并发/性能/泄漏/数值，五维 30 项）
python tools/sysinformer-accept/prod_accept.py \
  utils-support-native-sysinformer/src/main/resources/native/windows-x86_64/sysinformer.dll
#   -> PROD_ACCEPT_OK

# 2) 与任务管理器同源数据逐项对照（Windows）
python tools/sysinformer-accept/taskmgr_compare.py <同一个 dll>
#   -> TASKMGR_COMPARE_OK

# 3) CPU 口径判定（Windows；判定必须用这个，见下）
python tools/sysinformer-accept/cpu_windowed_compare.py <同一个 dll> 90
#   -> TASKMGR_CPU_WINDOWED_OK

# 4) Linux 生产验收（普通用户与 root 各跑一遍，权限相关行为不同）
python tools/sysinformer-accept/prod_accept.py <libsysinformer.so> --platform linux

# 5) Linux 电池取值验收（在 Linux 主机上跑，需 sudo + paramiko）
#    用 mount --bind 把可控目录覆盖到 /sys/class/power_supply，
#    因此不需要真笔记本也能验证取值分支
export SI_SSH_PASSWORD='...'
export SI_SO_PATH='<本地 libsysinformer.so 路径>'
python tools/sysinformer-accept/battery_value_linux.py <host> <user> /tmp/sysinf/libsysinformer.so
#   -> KALI_BATTERY_VALUE_OK（3 用例：字段齐全 / 缺 power_now 退回内核值 / 多电池+状态别名+非电池过滤）

# 6) Windows 笔记本 / MacBook 的**真机**对账（未验项 #6 的最后一步）
#    在**有电池的设备**上跑；无电池机器会明确输出 SKIPPED 而非静默通过
python tools/sysinformer-accept/battery_verify_device.py \
  utils-support-native-sysinformer/src/main/resources/native/windows-x86_64/sysinformer.dll
#   -> BATTERY_DEVICE_OK / BATTERY_DEVICE_FAILED
#   （无电池时）-> BATTERY_DEVICE_SKIPPED_NO_BATTERY

# 7) 真机对账脚本自身的参考源解析自测（本机非 macOS 时也该跑）
python tools/sysinformer-accept/battery_verify_device.py --selftest
#   -> SELFTEST_OK
```

**验证必须打在入库产物上**（`src/main/resources/native/<平台>/`），
不要用本地 `target/release` 的重建产物 —— 后者可能与交付物不一致。

**判定 CPU 口径必须用 `cpu_windowed_compare.py`。**
本库每次 refresh 的窗口约 27ms，PDH 的 `CookedValue` 窗口约 1s；
两侧量级差 50 倍时逐次差标准差约 14pp，逐点比较只会随机红绿。
`cpu_aligned_compare.py` 是瞬时配对版本，保留作参考，**不用于判定**。

对照方法自身的坑（都会造成假结论）完整清单见
`tools/sysinformer-accept/README.md`，其中最容易踩的两条：
用自己墙钟给 PDH 样本打时间戳会整体偏移 1~2.8s；
`GetSystemTimes` 不能当 CPU 基准（其 kernel 含 idle，会造出 30+pp 假偏差）。

## 九、交付物与被验收产物的一致性（2026-10-02 新增）

前面所有验收证据都是针对 **CI 编出来的二进制**取得的。而调用方实际拿到
的是**仓库里入库的那一份**。这两者相等，靠的不是「应该相等」，而是逐字节
比对 —— 否则就可能出现「交了全绿报告、交付的却是回退版」。

### 1) 曾发生的真实缺陷：四平台入库产物全部落后于源码

自查时发现 `main` 上的四份产物停在提交 `10063a6`（2026-10-01），此后三个
提交改了 Rust 源码（`c38dc54` 电池排序、`35e5b70` Windows CPU 修复），
**产物没有重新入库**：

| 平台 | 落后版本 | 缺少的修复 |
|---|---|---|
| windows-x86_64 | 847,360 `d4be17fa` | CPU `CStatus` 检查 + 100ms 最小采集间隔 |
| linux-x86_64 | 1,569,840 `18f6af72` | `battery.list` 排序 |
| darwin-x86_64 | 1,117,840 `fb3284ee` | `battery.list` 排序 |
| darwin-aarch64 | 1,121,616 `76abd383` | `battery.list` 排序 |

**为什么此前没被发现**：早期只对 Linux 的 `.so` 做过 md5 核对，
**两个 dylib 一次都没核过**。所以「四平台入库产物已验证」这句话对 macOS
当时并不成立。已从 artifacts 分支回填（提交 `3c72554`）。

### 2) Windows 构建原本不可复现（已修）

回填后做闭环核对，发现三平台逐字节相同、**只有 Windows 不同**。逐字节定位
后确定：852,480 字节里只差 **24 字节**，且 `.text`（605,184 字节可执行代码）
**差异为 0**；差异是 COFF `TimeDateStamp` 与 CodeView(RSDS) 调试 GUID，
两者都随**链接时刻**变化（MSVC 未开 `/Brepro`）。

后果是硬的：**无法用 md5 证明「交付的那份 == 验过的那份」**，而这是验收
报告绑定产物的唯一硬凭据（Linux/macOS 三个平台当时可以，只有 Windows 不行）。

已在 `build.sh` 对 MSVC 目标加 `-C link-arg=/Brepro`。

### 3) ⚠️ 上一版这里的「可复现性已被实测证明」是**错的**（2026-10-03 更正）

原文写的是「同 SHA 两次构建差异 0」，并据此断言四平台构建可复现。
下面这段保留 2026-10-02 的原始数据作为**历史记录**，但结论已被推翻。

**当时的实测（仅 Windows）**：

| 来源 | run / attempt | md5 | 字节 |
|---|---|---|---|
| pre-`/Brepro` | 36990756125 a1 | `f5e6a8d81cdcdf22e9dbb1c00e7ff977` | 852,480 |
| `/Brepro` 第 1 次 | 36995361316 a1 | `07cfbee40e4f2e9ca19e16319d961b8f` | 852,480 |
| `/Brepro` 第 2 次 | 36995361316 a2 | `07cfbee40e4f2e9ca19e16319d961b8f` | 852,480 |

`/Brepro` 本身是有效的：pre-`/Brepro` 与 `/Brepro` 那两份差 90,496 字节，
逐项查证为 2,478 个 `RUNTIME_FUNCTION` 只改 `UnwindInfoAddress`（`Begin`/`End`
零变化）、`.text` 那 8 字节是两条 `lea` 的 rip 相对位移（操作码相同），
即链接期生成的展开信息落位变了、代码语义不变。**这一条结论至今成立。**

**但「构建可复现」这个结论不成立。** 原因见下一节。

### 4) 构建此前**按构造**就不可复现：两个未固定的输入（2026-10-03 实测）

2026-10-03 做例行核对时，`verify_delivered.py` 报四平台**全部 MISMATCH**。
先排除干扰项：远端 main == 本地 HEAD（没被别人推走）、`src/main/rust` 与
`build.sh` 自 10-02 起**无任何改动**、工作区干净。即**输入逐字节相同，
输出却不同**。逐字节定位确认差异落在**代码段**：

| 平台 | 差异 |
|---|---|
| Windows `.text` | 22,939 / 605,184 字节 |
| macOS x86_64 `__TEXT` | 41,396 / 827,392 字节 |
| linux `.so` | 1,584,048 →（新构建）1,569,176，**体积变了** |
| macOS arm64 | 1,122,928 →（新构建）1,119,664，**体积变了** |

体积与 codegen 都变 —— 不是链接期元数据，是**编译器/依赖换了**。查出两个
独立的漂移源：

1. **`.gitignore` 第 11 行 `**/Cargo.lock`** —— 锁文件被刻意排除。而
   `Cargo.toml` 里 `sysinfo = "0.33"` / `serde = "1.0"` /
   `once_cell = "1.20"` / `libc = "0.2"` / `windows = "0.58"` /
   `wmi = "0.18"` 全是**开放版本范围**，锁文件不入库就意味着 CI 每次
   构建都重新解析 **90 个传递依赖**。
2. **完全没有工具链固定** —— 无 `rust-toolchain.toml`、`build.sh` 不钉
   版本、workflow 不装 Rust，全跟随 runner 预装的 stable；GitHub runner
   镜像滚动更新会带上新的 stable。

**上一版结论错在哪**：当时做了 6 次核对（同 SHA 两次 + 换 SHA 四次）都相同，
看起来远超"两次"的最低要求。但**短时间窗口内的多次一致不能证明长期可复现** ——
那 6 轮恰好落在依赖没更新的时间段里。当时只固定了 `/Brepro` 这一个输入
（链接器行为），却当成了全部；真正决定字节的两个输入（工具链、依赖解析）
当时都是浮动的。

**教训**：可复现性取决于**所有**输入是否固定，这是可查的事实，不依赖统计。
在输入没固定之前做多少次一致性核对都不构成证据。

### 5) 修复与**在固定输入下**的可复现性证明

三处改动，**缺一不可**：

1. `.gitignore` 给本模块 `Cargo.lock` 开例外（交付的是 cdylib，锁文件应入库）
2. 新增 `rust-toolchain.toml`，`channel = "1.97.1"`
3. **`cargo build --locked`** —— 最关键的一条。只入库锁文件而不加它
   等于白做：cargo 仍会按开放范围**静默更新**锁文件，且没有任何提示
4. `build.sh` 每次打印 `rustc --version` / `cargo --version`（四平台共用
   入口、本地也走它，于是每次构建日志自带版本）

版本钉子生效确认（run 37124428590 日志，四平台一致）：

```
rustc 1.97.1 (8bab26f4f 2026-07-14)    cargo 1.97.1 (c980f4866 2026-06-30)
```

**证明（run 37124428590，同一 SHA `43de392` 的两次独立构建）**：

| 组 | 内容 | 结果 |
|---|---|---|
| A | attempt-1 vs attempt-2，**四平台** | 各 **0 字节差异** |
| B | 钉版本**之前**的构建 vs 现在（四平台） | 长度全变 → **对照组能测出差异** |
| C | 2026-10-02 那批入库产物 vs 现在（四平台） | 长度全变 |

**B 组是必需的**：没有它，A 组的「相同」可能只是比对方法失灵。
2026-10-02 那次正是在这里翻的车 —— 当时也有敏感性对照，但对照的是
`/Brepro` 前后的差异（确实能测出差异），却没测「输入未固定时会不会不同」，
而后者才是真正的问题。

与 10-02 那次的另一个关键区别：**这次四平台全部纳入**。上次只测了 Windows，
而实际漂移最先发生在 linux/macOS（体积都变了）。

### 6) 当前可绑定的四平台指纹

| 平台 | md5 | 字节 |
|---|---|---|
| windows-x86_64 | `e43ee1b42542e02cc33f5c53a3bb3429` | 850,944 B |
| linux-x86_64 | `8502df90fe3a221f18fe11fc4dee3301` | 1,569,176 B |
| darwin-x86_64 | `f280d6d5b432514dabefb56147442fa5` | 1,124,380 B |
| darwin-aarch64 | `b6653cd845d7ce52b531015bf1cade9d` | 1,119,664 B |

来源 run 37124428590 attempt-1（与 attempt-2 逐字节相同）。

**换包即失效**：上面任一产物被替换后，本文件的验收数据都不再适用，
须重跑。核对用：

```bash
python tools/sysinformer-accept/verify_delivered.py <run_id>
#   -> DELIVERED_VERIFIED        （四平台逐字节与该 run 的 artifact 一致）
#   -> DELIVERED_MISMATCH        （退出码 1，至少一个平台不一致）
```

`run_id` 必须是结论为 `success` 的那一轮；脚本会拒绝用失败的 run 比对。

---

## 十、结论

- **"能用"口径：通过。** 四平台产物入库、架构与导出均已核验；**四平台全部有运行时验证**
  （Windows 本机、Linux 真实机器、macOS arm64 与 **Intel x86_64** 均 CI 真跑）；
  Java 8 与 Java 25 双基线都能跑通。
- **数值正确性口径：Windows 已用任务管理器同源数据逐项对照，12/12 通过**（CI 日志实测「判定项 = 12，失败 = 0」）。
  本轮因此修掉三个此前无人发现的缺陷：Windows 句柄少报约 80%、首次调用 CPU 报 100%、
  Windows CPU 口径与任务管理器差 +2.96 ~ +5.17pp。三者**编译、类型检查、
  30 项生产验收、冒烟测试全部发现不了**。
- **四平台生产验收：全部通过**（run 36995361316，sha `2deb08a`，结论 `success`）。
  linux 33/33、windows 33/33、darwin-arm64 31/31、darwin-x86_64 31/31
  （macOS 少 2 项是 `events.*` 硬限制，日志有明确说明）。
  CPU 判据：有效配对 55，均值差 +0.795pp ≤ 4.0pp，`TASKMGR_CPU_WINDOWED_OK`。
- **交付物与被验收产物一致：已用逐字节比对证明**（见第九节），
  且 Windows 产物经两次独立构建复现，四个平台现在都能用 md5 绑定。
  此前「四平台入库产物已验证」对 macOS 与 Windows 都不成立，已修正。
- **"生产级"口径：三平台（Windows/Linux/macOS）生产验收全绿，
  四平台运行时冒烟全覆盖。**
  仍未落实的未验项：**#2**（Java 25 FFM 绑定进 CI，需私有仓库凭据，属全仓性限制）、
  **#6**（Windows/macOS 电池取值分支需真机，测试环境无电池设备）、
  **#7**（CPU 判定为统计性，95% CI 含 0 而非逐点相等）；
  #3 为硬限制且已豁免；#4 / #5 属独立立项。
  **在 #2 与 #6 落实或明确豁免之前，不宣告"生产级已全部验收"。**

### 一句话回答"数据准不准、比不比任务管理器准"

**对齐口径后与任务管理器一致；对齐之前会系统性偏高。**

对齐后（同一 PDH 数据源）实测偏差：内存 0.00%、进程数 0.00%、磁盘 0.00%、
线程 0.03%、句柄 0.06%，CPU 在统计意义上无系统性偏差。

但口径不对齐就会偏——本轮的 CPU 就是活例子：`sysinfo` 的 `100 - %Idle`
比任务管理器的 `% Processor Time` 高约 3~5pp，20 组采样下 95% 置信区间
不含 0，是真实缺陷而非噪声。所以"本模块数据与任务管理器一致"是
**逐项对照后的结论，不是设计前提**。