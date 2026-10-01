# sysinformer-accept —— 生产级验收脚本

对 `utils-support-native-sysinformer` 的**入库产物**做生产级验收。这组脚本回答的是
"能用"之上的问题：并发安全、性能、资源泄漏、边界负例、数值正确性。

验收结论与未验项见模块目录下的 `ACCEPTANCE.md`。

## 用法

```bash
# Windows
python prod_accept.py <入库的 sysinformer.dll> --platform windows

# Linux（普通用户与 root 各跑一遍，权限相关能力的行为不同）
python prod_accept.py <入库的 libsysinformer.so> --platform linux
```

脚本会打印断言明细、性能分位表，并以 `PROD_ACCEPT_OK` / `PROD_ACCEPT_FAILED` 收尾。

## 与任务管理器逐项对照（Windows）

```powershell
python taskmgr_compare.py <入库的 sysinformer.dll>
```

以 `PROD_ACCEPT_OK` 之外独立收尾的 `TASKMGR_COMPARE_OK` / `TASKMGR_COMPARE_FAILED` 标记。

**对照源必须是任务管理器的同源数据**（PDH `\Processor(_Total)\% Processor Time`
与 `GetPerformanceInfo`），而不是肉眼比对。2026-10-01 实测 15 项全绿。

## 五个维度

| 维度 | 内容 | 要求 |
|---|---|---|
| 1 边界与负例 | 非法 op / 空 op / 超长 op / 畸形 JSON / 缺参 / pid 为负或超大或字符串 / 未启动就 poll / 未知动作 等 18 项 | 全部返回**合法信封**（`ok:false` + 原因），**无一崩溃**（FFI 崩溃会带走宿主 JVM）|
| 2 并发安全 | 8 线程 × 15 轮 × 6 op 混合调用；事件订阅下 4 线程并发 poll | 无非法信封、无崩溃、结果一致 |
| 3 性能基线 | 每 op 30 次，报 p50/p95/max | 供容量规划 |
| 4 资源泄漏 | 200 轮 × 4 op 后比对 RSS 与句柄/fd；事件启停 20 轮后比对句柄 | 不线性增长 |
| 5 数值对照 | 进程数 / 逻辑核数 / 内存总量 / 自身 RSS，与系统工具交叉核对 | 差异在合理范围。这是"返回了结构但数字是错的"的唯一防线 |

## 两个必须注意的自身缺陷（本脚本开发中踩过）

1. **Windows 侧采集内存/句柄必须显式设 `argtypes`/`restype`**。
   `GetCurrentProcess()` 的默认返回类型是 32 位 int，在 x64 上会把 64 位句柄**截断**，
   于是后续调用全部失败、采集返回 -1，**让泄漏检查变成"0 比 0"的空转断言**。
2. **负例里 `events.start` 可能真的把会话建起来**，测完必须 `events.stop`。
   否则后面的"事件订阅下并发 poll"会因为"已在运行"而整段被跳过。
   脚本里另有一步"采集可用性自检"，为 0 就直接判失败，防止泄漏检查空转。

## 为什么必须跑真实机器

本脚本至今抓出的问题**全部是编译与类型检查发现不了的**，例如：

- Linux netlink 的 `nl_groups` / `nlmsg_type` 写错 → 订阅"成功"但 0 事件
- Windows `SERVICE_STATUS_PROCESS` 偏移错 → 297 个服务状态全 unknown
- Linux DMI 结构解析 off-by-one → 内存条永远读不到
- sysinfo 把线程当进程 → 进程数虚高 6 倍
- Windows 句柄表用了已废弃的 `SystemHandleInformation`(类号 16) → **少报约 80%**，
  47 个进程"明明有句柄却返回空"（真实 stride 是 24 字节而非代码假设的 20 字节）
- 进程内首次 `system.snapshot` 的 CPU 全核报 100%（`System::new()` 无基线）

因此**不要用 `cargo check` 通过代替本脚本**，也**不要用编译通过代替
`taskmgr_compare.py`**——后四项连 30 项生产验收都能全绿地通过。

## 对照脚本自身的六个坑（2026-10-01 全部踩过）

这些不是被测代码的缺陷，而是**对照方式**出错，一度让我误判成"库有 bug"：

| 坑 | 表现 | 正确做法 |
|---|---|---|
| 用 `GetSystemTimes` 当 CPU 基准 | 造出 **30+pp** 假偏差 | 它的 kernel 时间含 idle，与 PDH `% Processor Time` 口径不同 |
| api 与 ref 之间隔着遍历 300+ 进程 | 内存差 6.28%、磁盘差 3.8% | 紧邻采样，或多次取均值 |
| `Get-Counter` 逐核查 12 个核 | 连 PDH 自身都不满足 `_Total ≈ 每核和/核数` | 每次调用是独立窗口，只比"汇总 vs `_Total`" |
| 单轮 CPU 差值判 FAIL | 同条件两轮分别 2.08pp / 11.96pp | 交替采样 + 看均值与符号分布 |
| `ctypes.wintypes.BYTE` 是有符号 | `BatteryFlag=128` 打成 `-128`，误判"有电池" | 显式 `c_ubyte` |
| PDH 结构体给 128/512 字节 | 一律 `PDH_INVALID_DATA` | `PDH_H_QUERY` 变长不透明，改用 PowerShell `Get-Counter` |
| **用自己墙钟给 PDH 样本打时间戳** | 整体偏移 1~2.8 秒，配对到错误时刻 | `Get-Counter` 内部耗时 1000~2840ms（实测），`CookedValue` 描述的是**调用结束时**那段窗口。必须用计数器自报的 `$s.Timestamp` |
| **27ms 窗口 vs 1s 窗口直接比** | 逐次差标准差 14pp，四轮均值乱摆 | 把本库在 PDH 窗口内的多样本取均值，让两侧被测区间拉平 |
| 自洽性差值恒为 0 时判 CI 含 0 | 完美自洽被判成"存在偏差" | 浮点零使 `lo<=0<=hi` 在 `[+0.000,+0.000]` 上不成立，需容差 |
| 分别 `Get-Counter` 取 `% Processor Time` 与 `% Idle Time` 再比较 | 两者窗口不同，结论无效 | 多计数器要放同一次查询 |

## CPU 口径定案（Windows）

```powershell
python cpu_windowed_compare.py <dll> 90
```

`TASKMGR_CPU_WINDOWED_OK` / `TASKMGR_CPU_WINDOWED_FAILED`。

**背景**：任务管理器的 CPU 数字来自 PDH `% Processor Time`；而 `sysinfo`
在 Windows 上只读 `% Idle Time` 并取 `100 - idle`，两者分母不同，
实测系统性偏高 +2.96 ~ +5.17pp。本模块已改用前者（`src/cpu_windows.rs`）。

`cpu_aligned_compare.py` 是瞬时配对版本，保留作参考；**判定以
`cpu_windowed_compare.py` 为准**（方差小得多）。

反向对照已验证：把口径切回旧实现后，同一判据稳定 FAILED。