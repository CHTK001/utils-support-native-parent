---
name: ch-example-test-system
description: 为 utils-support-example-starter 的 Example 类建立"example-starter 测试体系"。当用户要求复测/补充某个 Example 示例的单元测试、或创建 *Test 测试类时使用。覆盖 CH Java 编码规范、maven-compiler-plugin 的 includes 白名单、Assumptions 跳过约定，以及 SMB / FileSearch / HTTP 等示例的具体 API 与依赖。
agent_created: true
---

# CH Example 测试体系

## 适用场景

- 用户要求"复测 FileSearchExample / SmbServerExample / SmbClientExample / HttpServerExample 等"或"为某 Example 创建/完成测试"。
- 规则约定：若 Example 已存在直接测试则运行它；若不存在（含 Example 类本身不存在）则按 example-starter 体系创建 Example（仅 `main`）+ 自包含 `*Test`。

## 核心约定（已与用户确认，必须遵循）

1. **example 体系 = `XxExample` 类 + `main` 方法**。Example 类只负责"演示用法"，**不写 `runTest` 之类的自检方法**——自检逻辑属于测试，应放在 `*Test` 里。
2. **测试自包含**：`*Test` 直接调用底层 API（如 `FileSearchService`、`SmbServer`/`SmbClient`、`ServerBuilder`）完成验证，**不依赖 Example 类暴露的方法**。
3. **优雅跳过依赖/native**：用 `org.junit.jupiter.api.Assumptions` 在缺失依赖时跳过，避免无 native/无依赖的 CI 误报：
   - 可探测可用性的（FileSearch：`service.isAvailable()`）：`Assumptions.assumeTrue(available, "...")`。
   - 仅运行期才知道是否可用的（SMB / HTTP 各实现）：`try { ... } catch (Exception e) { Assumptions.assumeNoException("...", e); }`。
   - 不要再使用 `@Disabled` 长期禁用（除非确属外部资源不可用）；当前已对齐为 Assumptions 跳过。
4. 遵循 CH Java 编码规范（`/ch-java-coding-style`）：中文详细注释、类注释带 `@author CH`、控制语句必须用大括号、禁止代码压缩、优先用 Lombok/Record。

## maven-compiler-plugin 白名单（关键坑）

`pom.xml` 中 `maven-compiler-plugin` 配置了 `<includes>` 白名单，**只有列出的文件才会被编译**；该白名单同时作用于compile 与 testCompile。因此：

- 新增的 main Example 类（如 `SmbServerExample.java`）必须加入 `<includes>`，否则不编译。
- 测试类也必须加入 `<includes>`（如 `SmbServerExampleTest.java`），否则不会被 testCompile 编译。
- 目前白名单已含：`SafetensorsFaceDetectionExample`、`FileSearchExample`、`FileSearchAllExample`、`FileSearchHomeExample`、`HttpServerExample`、`WinRmExample`、`WinRmClientExample`、`SmbServerExample`、`SmbClientExample` 及对应 `*Test`。

## 依赖

- example-starter 已依赖：`utils-support-common-starter`、`utils-support-filesearch-starter`、`utils-support-network-starter`、`utils-support-deeplearning-*`、`utils-support-remote-starter`、`utils-support-winrm-starter`、`utils-support-ssh-starter`。
- **SMB 不在上述传递依赖中**：`utils-support-smb-starter` 必须显式添加：
  ```xml
  <dependency>
      <groupId>com.chua</groupId>
      <artifactId>utils-support-smb-starter</artifactId>
      <version>${project.version}</version>
  </dependency>
  ```
- 构建前需先把 `utils-support-smb-starter` 装入本地 `.m2`（它依赖第三方 `com.hierynomus:smbj`，默认不在仓库）：
  `mvn -pl utils-support-smb-starter -am install -DskipTests`
- 本机 Maven（`/d/apache-maven-3.9.9`）执行 `mvn -v` 也会挂起，无法在此环境编译验证，需用户在自有环境构建。

## SMB 示例 API（来自 utils-support-smb-starter）

- 服务端：`com.chua.smb.server.SmbServer`，构建器 `SmbServer.builder().host().port().shareName().rootPath().user().password().build()`；`server.start()` / `server.isRunning()` / `server.stop()`。
- 客户端：`com.chua.smb.client.SmbClient`（实现 `AutoCloseable`），链式 `SmbClient.create("smb://user:pass@host:port/share").connect().login().openShare()`；`client.listFiles("/")` 返回 `List<SmbFileEntry>`（内部 record）。
- 两者底层均依赖 Rust native 库 `rust_smb_server`（由 `RustSmbServerBridge` 从 `utils-support-native-smb` 的 jar 资源中加载）。
- **测试使用非特权高位端口（服务端 18445、客户端回环 18446），不要占用 445**；`try/finally` 确保 `server.stop()`。
- `SmbClient` 可用 try-with-resources（`implements AutoCloseable`）。

## FileSearch 示例 API（来自 utils-support-filesearch-starter）

- `com.chua.filesearch.support.service.FileSearchService.getInstance().isAvailable()` 判断 native 是否就绪。
- `FileSearchCriteria.builder().rootPath().namePattern().maxResults().sortBy(SORT_BY_NAME).order(ORDER_ASC).build()`；`service.search(criteria)` 返回 `List<FileInfo>`（`info.path()`）。
- 测试在临时目录建 `demo.java`/`readme.md` 后搜索 `*.java`，断言结果含 `.java`，并用 `Assumptions.assumeTrue(isAvailable())` 跳过。

## HTTP 示例 API（来自 utils-support-common-starter）

- `com.chua.common.support.network.server.ServerBuilder.create().type(type).port(port).build()`；`server.start()` / `server.isRunning()` / `server.stop()`。
- `type` 可选：`jdk`、`jdk-http`、`netty-http`、`vertx-http`、`rust-tokio`。`port(0)` 表示自动分配，避免端口冲突。
- 测试对每个 type 调 `assertStartStop(type)`，用 `Assumptions.assumeNoException` 在缺少对应实现依赖时跳过。

## 落地检查清单

1. 确认目标 Example 是否真实存在（Glob + Grep `class XxxExample`）；不存在则按用户指示新建 Example 类（**只写 `main`**）。
2. 在 `src/test/java/com/chua/example/<area>/` 建自包含 `*Test`（**直接调 API**，不调 Example 方法），轻量用 `assumeTrue`、运行期才知可用性用 `assumeNoException` 包裹 `start()` 等。
3. 更新 `pom.xml`：补依赖（SMB 必需）+ 把新 main 类与测试类加入 `maven-compiler-plugin` 的 `<includes>`。
4. 提示用户在自有环境 `mvn install` 依赖并 `mvn test` 验证（本机 Maven 不可用）。
