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
| windows-x86_64 | `sysinformer.dll` | 本机 Windows 真跑（ctypes + Java 25 FFM 冒烟）|
| linux-x86_64 | `libsysinformer.so` | **真实 Kali 机器**（Kernel 6.19 / x86_64），普通用户 + root 各一遍 |
| darwin-aarch64 | `libsysinformer.dylib` | CI（macos-15，arm64 原生）真 dlopen + 冒烟 |
| darwin-x86_64 | `libsysinformer.dylib` | ⚠️ **仅导出 + 架构断言**，无运行时冒烟（见未验项）|

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

### Windows（`sysinformer.dll` md5 `269dbb0b…`）

```
通过 30 / 失败 0   PROD_ACCEPT_OK
进程数对照：api=342 vs tasklist=343      逻辑核 12 vs os.cpu_count 12
内存 34.22GB vs 系统 34.22GB（差 0.0%）   自身 RSS 差 2%
并发 8 线程 × 15 轮 × 6 op：8.28s，无非法信封
泄漏：200 轮后 RSS +0.4MB、句柄 +5；事件启停 20 轮后句柄 +0
```

### Linux（`libsysinformer.so`，真实 Kali）

```
普通用户  通过 30 / 失败 0   PROD_ACCEPT_OK
root      通过 30 / 失败 0   PROD_ACCEPT_OK
进程数对照：api=206 vs ps=207
并发 8 线程 × 15 轮 × 6 op：10.8s / 18.8s，无非法信封
泄漏：200 轮后 RSS -4MB、句柄 -11；事件启停 20 轮后句柄 +0
```

### 性能（Windows，p50）

| op | p50 | 说明 |
|---|---|---|
| system.snapshot | **29 ms** | 见下方"性能修复" |
| process.list | 26 ms | |
| process.detail | 13 ms | |
| process.tree | 17 ms | |
| kernel.modules | 0.8 ms | |
| socket.list | 2.4 ms | |
| process.env | 0.2 ms | |
| process.modules | 0.7 ms | |

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
| 1 | **macOS x86_64 无运行时冒烟** | macos-15 runner 只有 arm64，无法 dlopen x86_64 dylib | 该腿只有导出与架构断言；要真验需一台 Intel Mac |
| 2 | **Java 25 FFM 绑定不进 CI** | `SysInformerNative` 依赖 `utils-support-common-starter`，该构件位于 packages.aliyun.com **私有**仓库（匿名 401）| 配置 `MAVEN_ALIYUN_USER` / `MAVEN_ALIYUN_PASSWORD` 后纳入 `native-java-compile.yml`；目前只有本地验证 |
| 3 | **macOS `events.*`** | 系统级进程事件需 EndpointSecurity 框架及其 Apple 授权 entitlement（`com.apple.developer.endpoint-security.client`），只签发给经 Apple 批准的签名应用 | 硬限制。代码里明写"**不以轮询伪装成事件**" |
| 4 | **未做真实业务集成测试** | 属独立立项 | 本模块只保证"库本身可用且指标数值正确" |
| 5 | **未做长时间稳定性压测** | 属独立立项 | 目前只有 200 轮量级的泄漏检查 |

---

## 七、可复现的验证命令

```bash
# 编译（本机 Windows 可用 gnu toolchain；交叉目标做类型检查）
cargo +stable-x86_64-pc-windows-gnu build --release
cargo +stable-x86_64-pc-windows-gnu check --target x86_64-unknown-linux-gnu
cargo +stable-x86_64-pc-windows-gnu check --target x86_64-apple-darwin
cargo +stable-x86_64-pc-windows-gnu check --target aarch64-apple-darwin

# 生产级验收（仓库入库的产物，不是本地重建）
python prod_accept.py <入库动态库路径> --platform windows
python prod_accept.py <入库动态库路径> --platform linux      # root 与非 root 各跑一遍

# Java 25 侧冒烟（需先 mvn compile）
javac -encoding UTF-8 -cp "target/classes;<cp>" -d target/smoke src/smoke/java/FfmSmoke.java
java --enable-native-access=ALL-UNNAMED -cp "target/smoke;target/classes;<cp>" FfmSmoke
```

---

## 八、结论

- **"能用"口径：通过。** 四平台产物入库、架构与导出均已核验；三平台有运行时验证
  （Windows 本机、Linux 真实机器、macOS arm64 CI）；Java 8 与 Java 25 双基线都能跑通。
- **"生产级"口径：Windows 与 Linux 达标（各 30/30），但整体不完整** ——
  未验项 1 与 2 需要额外资源（Intel Mac / 私有仓库凭据），未验项 3 是硬限制。
  **在这三项未落实或未明确豁免之前，不应宣告"生产级已验收"。**