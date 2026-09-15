# utils-support-native-wechat

微信 4.x WCDB 原生读取库（自研，跨平台替代闭源 `wcdb_api.dll`）。

## 概述

微信 4.x 的本地数据库基于 WCDB（SQLCipher 4）：AES-256-CBC + HMAC-SHA512，页大小 4096，PBKDF2-HMAC-SHA512 迭代 256000 次。本模块提供 8 个 C ABI 接口，以 64 位十六进制原始密钥直接解密读取，不依赖微信进程或任何闭源动态库。

## 模块结构

```
utils-support-native-wechat/
├── pom.xml
├── src/
│   ├── main/java/com/chua/nativewechat/
│   │   ├── support/WechatWcdbBridge.java       # Java FFM 桥接器
│   │   └── smoke/WechatWcdbSmokeTest.java      # 端到端冒烟测试
│   ├── main/rust/                              # Rust FFI 库
│   │   ├── Cargo.toml
│   │   ├── build.sh
│   │   └── src/
│   │       ├── lib.rs                          # 导出 8 个 C ABI 函数
│   │       └── bin/selftest.rs                 # Rust 自测
│   └── main/resources/META-INF/native-image/
│       └── com.chua/utils-support-native-wechat/
│           ├── native-image.properties          # GraalVM native-image 配置
│           └── resource-config.json             # GraalVM 资源反射配置
```

## 平台支持

当前预编译动态库随 jar 打包，支持：

- `windows-x86_64`：`wechat_wcdb.dll`
- `linux-x86_64`：`libwechat_wcdb.so`

`WechatWcdbBridge.load()` 时自动从 classpath 抽取并按平台加载，无需外部配置原生库目录。

## 构建

### 编译 Rust 动态库

```bash
cd src/main/rust
./build.sh
```

脚本会交叉编译 `wechat_wcdb.dll`（Windows）与 `libwechat_wcdb.so`（Linux），产物放置于 `src/main/resources/native/{platform}/`。

### Maven 编译

```bash
mvn clean install -pl utils-support-native-parent/utils-support-native-wechat
```

需要 Java 25 编译插件与 `-enable-native-access` / `--enable-preview` 参数（已在 `pom.xml` 中配置）。

## 使用

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

## C ABI 接口

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

## 冒烟测试

```java
com.chua.nativewechat.smoke.WechatWcdbSmokeTest
```

支持参数：

- 无参数 — 自动遍历 `key_info.db` 全部候选密钥
- `64位raw_key_hex` — Rust 侧自动拼接 salt
- `96位完整密钥_hex` — 直接传入

## 与闭源 wcdb_api.dll 的区别

- 无 `WCDB.dll` / `SDL2.dll` 依赖，无 `electron.exe` 宿主进程名校验，普通 JVM 直接可用。
- 跨平台（Windows / Linux），错误信息可通过 `lastError()` 获取。
- 打开失败或查询失败时返回非 0 码，调用方可读取 `lastError()` 定位原因。
