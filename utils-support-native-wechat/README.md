# utils-support-native-wechat

微信本地数据与界面自动化能力集：WCDB 原生读取（4.x）+ UIA 会话轮询（3.9.x）。

## 概述

本模块提供两条互补的能力线：

| 能力 | 实现 | 适用版本 | 方向 |
|------|------|----------|------|
| WCDB 消息读取 | 自研 Rust + SQLCipher 动态库 | 微信 **4.x** | 只读 |

> 原先的「UIA 会话轮询与回信（3.9.x）」通道已随 `utils-support-native-uia` 一并退役。
> 本模块现在只保留 WCDB 直读一条通道，因此不再需要区分 3.9.x 与 4.x 的控件树差异。

## 模块结构

```
utils-support-native-wechat/
├── pom.xml
├── src/
│   ├── main/java/com/chua/nativewechat/
│   │   └── support/
│   │       ├── WechatWcdbBridge.java       # WCDB Java FFM 桥接器
│   │       └── WechatKeyExtractor.java
│   ├── main/rust/                           # WCDB FFI 库
│   └── main/resources/native/
└── src/test/java/com/chua/nativewechat/smoke/
    ├── WechatWcdbExample.java
    ├── PipelineExample.java
    └── KeyExample.java
```

## 平台支持

当前预编译动态库随 jar 打包，支持：

- `windows-x86_64`：`wechat_wcdb.dll`
- `linux-x86_64`：`libwechat_wcdb.so`
- `darwin-x86_64`：`libwechat_wcdb.dylib`
- `darwin-aarch64`：`libwechat_wcdb.dylib`

`WechatWcdbBridge.load()` 时自动从 classpath 抽取并按平台加载，无需外部配置原生库目录。

### 已知不一致：Windows 产物缺 2 个密钥导出

`src/main/rust/src/lib.rs` 声明了 10 个 `#[no_mangle]` 导出，其中
`wechat_wcdb_extract_key` 与 `wechat_wcdb_can_extract_key`
（进程内存取密钥，仅 Windows 有效）在**已提交的 `windows-x86_64/wechat_wcdb.dll` 中不存在**——
该 DLL 是从早于这两个函数加入的源码构建的。linux 与两个 darwin 产物均包含这两个符号。

影响范围有限，但需明确：

- **当前无运行期影响**。密钥提取的**实际调用路径是纯 Java 实现**
  `WechatKeyExtractor`，它用 FFM 直连 `kernel32.dll`
  （`CreateToolhelp32Snapshot` / `OpenProcess` / `ReadProcessMemory`），
  自行扫描进程内存，**不经过 Rust 动态库**，因此不查这两个符号。
- **Rust 侧密钥提取在 Windows 上不可用**。直接以 FFM 调用
  `wechat_wcdb_extract_key` 会得到 `UnsatisfiedLinkError`；
  `wechat_wcdb_can_extract_key` 同样取不到。
- 修复方式：由 `.github/workflows/native-wechat.yml` 重建 windows-x86_64 产物
  （该 workflow 已在 windows-2022 上执行 `build.sh windows x86_64 release`），
  回填后即一致。本地重建不推荐：`bundled-sqlcipher` +
  `bundled-sqlcipher-vendored-openssl` 需从源码编译 SQLCipher 与 OpenSSL
  （需 Perl、nasm、VC 工具链，耗时很长）。

## 构建

### 编译 Rust 动态库（WCDB）

```bash
cd src/main/rust
./build.sh                      # 自动检测平台
./build.sh windows x86_64 release
```

### Maven 编译

```bash
mvn install -DskipTests
```

需要 Java 25 编译插件与 `-enable-native-access` / `--enable-preview` 参数（已在 `pom.xml` 中配置）。

---

## 一、WCDB 读取（4.x）

### Java 侧

```java
import com.chua.nativewechat.support.WechatWcdbBridge;

try (WechatWcdbBridge bridge = WechatWcdbBridge.load()) {
    String key = "64位hex原始密钥";
    long handle = bridge.openAccount("E:/微信/xwechat_files/wxid_xxx_b942/db_storage/session/session.db", key);

    String sessionsJson = bridge.getSessions(handle);
    String messagesJson = bridge.getMessages(handle, "wxid_yyy", 500, 0);
    int count = bridge.getMessageCount(handle, "wxid_yyy");
    String namesJson = bridge.getDisplayNames(handle, "[\"wxid_yyy\"]");

    bridge.closeAccount(handle);
}
```

### 密钥说明

- 传入 **64 位 hex raw key**：Rust 侧自动从 DB 文件头读取 16 字节 salt（32 位 hex），拼接为 96 位完整密钥。
- 传入 **96 位 hex 完整密钥**：直接使用。

### C ABI 接口

| 函数 | 说明 |
|------|------|
| `wechat_wcdb_open_account` | 打开并解密 session.db，自动 ATTACH 相邻 message / contact 分片库 |
| `wechat_wcdb_close_account` | 关闭账号库句柄并释放原生资源 |
| `wechat_wcdb_get_sessions` | 获取全部会话列表 JSON |
| `wechat_wcdb_get_messages` | 分页获取指定会话消息 JSON（跨分片 UNION ALL） |
| `wechat_wcdb_get_message_count` | 获取指定会话消息总数（跨全部分片求和） |
| `wechat_wcdb_get_display_names` | 批量解析发送者显示名称 |
| `wechat_wcdb_free_string` | 释放本库通过出参返回的字符串 |
| `wechat_wcdb_last_error` | 返回最近一次错误信息 |
| `wechat_wcdb_extract_key` | 从运行中的微信进程内存提取数据库密钥（**仅 Windows**，当前 Windows 产物缺失，见上文） |
| `wechat_wcdb_can_extract_key` | 查询当前平台是否支持密钥提取（**仅 Windows**，当前 Windows 产物缺失，见上文） |

### 与闭源 wcdb_api.dll 的区别

- 无 `WCDB.dll` / `SDL2.dll` 依赖，无 `electron.exe` 宿主进程名校验，普通 JVM 直接可用。
- 跨平台（Windows / Linux / macOS x86_64 / macOS arm64），错误信息可通过 `lastError()` 获取。
- 打开失败或查询失败时返回非 0 码，调用方可读取 `lastError()` 定位原因。


