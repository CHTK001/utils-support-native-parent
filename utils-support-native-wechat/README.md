# utils-support-native-wechat

微信本地数据与界面自动化能力集：WCDB 原生读取（4.x）+ UIA 会话轮询（3.9.x）。

## 概述

本模块提供两条互补的能力线：

| 能力 | 实现 | 适用版本 | 方向 |
|------|------|----------|------|
| WCDB 消息读取 | 自研 Rust + SQLCipher 动态库 | 微信 **4.x** | 只读 |
| UIA 会话轮询与回信 | 复用 `utils-support-native-uia` | 微信 **3.9.x** | 读 + 写 |

**版本不可混用**：Rust 侧 SQL 针对 4.x 表结构；UIA 控件树在 3.9.x（原生 Qt）上最稳。
4.x 换成 Electron 渲染后控件层级会变成大量 `Custom`，UIA 定位可靠性大幅下降。

## 模块结构

```
utils-support-native-wechat/
├── pom.xml
├── src/
│   ├── main/java/com/chua/nativewechat/
│   │   ├── support/
│   │   │   ├── WechatWcdbBridge.java       # WCDB Java FFM 桥接器
│   │   │   └── WechatKeyExtractor.java
│   │   └── uia/
│   │       ├── WechatUiaPollDirectory.java  # 会话轮询目录（监听 + 回信）
│   │       ├── WechatUiaSession.java        # 会话（含 reply 回信）
│   │       ├── WechatUiaMessage.java        # 消息事件
│   │       ├── WechatUiaReplyResult.java    # 回信结果
│   │       └── WechatUiaSelectors.java      # 控件定位模板
│   ├── main/rust/                           # WCDB FFI 库
│   └── main/resources/native/
└── src/test/java/com/chua/nativewechat/smoke/
    ├── WechatWcdbExample.java
    ├── PipelineExample.java
    ├── KeyExample.java
    └── WechatUiaSmokeTest.java              # UIA 端到端冒烟测试
```

## 平台支持

当前预编译动态库随 jar 打包，支持：

- `windows-x86_64`：`wechat_wcdb.dll`
- `linux-x86_64`：`libwechat_wcdb.so`

`WechatWcdbBridge.load()` 时自动从 classpath 抽取并按平台加载，无需外部配置原生库目录。

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

### 与闭源 wcdb_api.dll 的区别

- 无 `WCDB.dll` / `SDL2.dll` 依赖，无 `electron.exe` 宿主进程名校验，普通 JVM 直接可用。
- 跨平台（Windows / Linux），错误信息可通过 `lastError()` 获取。
- 打开失败或查询失败时返回非 0 码，调用方可读取 `lastError()` 定位原因。

---

## 二、UIA 会话轮询与回信（3.9.x）

基于 `utils-support-native-uia` 的通用 UIA 原语，**不注入、不改内存、不走私有协议**。

### 使用

```java
try (WechatUiaPollDirectory dir = WechatUiaPollDirectory.open()) {
    dir.markBaseline();          // 冷启动：不回溯历史，否则会把历史消息全答一遍
    while (running) {
        try (WechatUiaPollDirectory.PollBatch batch = dir.poll()) {
            for (WechatUiaSession session : batch.sessions()) {
                // session：标题 + 源用户 + 消息列表 + 时间
                String question = session.mergedContent(" ");
                String answer = llm.ask(question);
                WechatUiaReplyResult r = session.reply(answer);
                if (!r.isSuccess()) {
                    log.warn("回信失败: {}", r.getError());
                }
            }
        }
        Thread.sleep(2000);
    }
}
```

### 工作原理

1. 遍历左侧会话列表项，取每个会话的标题与未读数；
2. 对有新消息迹象的会话逐个打开，读取右侧消息列表；
3. 用去重键过滤已处理消息，聚合成 `WechatUiaSession` 返回；
4. 回信时切回目标会话，**校验标题一致后**才写入并提交。

### 关键约束

- **窗口必须可见且未最小化**：最小化时 Qt 侧不实例化内部控件，UIA 只能看到 3~4 个节点，读不到任何消息。
- **回信前强制校验标题**：UI 自动化只能靠界面上的*昵称文本*切会话，而昵称可能重复、撞名、被改动。
  切错就会把私聊内容发进错误会话，因此校验不过宁可不发。这条约束不可关闭。
- **非线程安全**，且 COM 单元与线程绑定，必须在同一线程内创建和使用。
- **回信会抢占窗口焦点**，调用方需自行处理与用户正常使用的并发。
- **精度上限**：UIA 读不到微信内部 msgId，`WechatUiaMessage#messageId()` 由
  "会话 + 发送者 + 可见文本" 派生，连发内容完全相同的多条消息会被视为重复。
  需要严格区分时应改走 WCDB 直读。

### 选择器调优

`WechatUiaSelectors` 把"微信控件在哪"从代码里抽成可替换配置。控件层级不准时：

```java
String tree = dir.dumpTree(20, 4000);
Files.write(Path.of("uia_tree.json"), tree.getBytes(UTF_8));
```

对照真实树修改 `WechatUiaSelectors` 各选择器即可，无需改动 `WechatUiaPollDirectory` 逻辑。

### 冒烟测试

```bash
# 端到端分阶段验证：动态库加载 → 窗口绑定 → 控件树 → 选择器 → 会话 → 轮询 → 回信
mvn test-compile
java --enable-native-access=ALL-UNNAMED --enable-preview \
  -cp "target/classes;target/test-classes;$(cat target/cp.txt)" \
  com.chua.nativewechat.smoke.WechatUiaSmokeTest

# 加 --reply 会真实发送一条消息，仅在测试号之间验证时使用
```

窗口最小化时测试会明确报出"节点过少"而不是静默通过。

