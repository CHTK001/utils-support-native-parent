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

因此**不要用 `cargo check` 通过代替本脚本**。