---
name: ch-java-coding-style
description: "CH Java 编码规范技能。涵盖 16 条强制规则：中文详细注释、属性多行注释、单行注释在代码上方、类注释添加 @author CH、控制语句必须大括号、禁止代码压缩、Lombok/Record 简化代码和日志、删除 package 上方注释、Record 注释在类上、修复代码问题、示例工程规范、P3C 完整遵守、示例统一架构（runTest+main 入口）、Lambda effectively final、示例 pom 同步 includes。当编写、审查或重构 com.chua 项目 Java 代码时使用此技能。"
---

# CH Java 编码规范

本技能定义 `com.chua` 项目（`utils-support-parent-starter`）所有 Java 源代码必须遵守的编码规范。
所有规则均为**强制**级别，违反者不得通过 Code Review。

---

## 规则总览

| 编号 | 规则名称 | 核心要求 |
|:----:|:--------|:--------|
| 1 | 中文详细注释 | 所有注释使用中文，语义清晰完整 |
| 2 | 属性必须多行注释 | 字段/常量使用 Javadoc `/** */` |
| 3 | 单行注释位置 | `//` 注释必须在代码上方，禁止行尾 |
| 4 | 类注释添加 @author CH | 所有类 Javadoc 必须包含 `@author CH` |
| 5 | 控制语句大括号 | if/else/while/for/do 必须使用大括号 |
| 6 | 禁止代码压缩 | 多行代码必须展开，禁止压成一行 |
| 7 | Lombok/Record 简化 | 使用 @Slf4j/@Data/record 简化代码 |
| 8 | 删除 package 上方注释 |  package 语句上方不得有任何注释 |
| 9 | Lombok 和 Record 简化 | 同规则 7，POJO 优先 record，日志优先 @Slf4j |
| 10 | Record 注释在类上 | record 的注释写在 record 声明处，不在各字段上 |
| 11 | 修复代码问题 | 消除魔法值、NPE 风险、资源泄漏等 |
| 12 | 示例工程规范 | 所有示例写在 utils-support-example-starter |
| 13 | 遵守 P3C | 完整遵守阿里巴巴 Java 开发手册（黄山版） |
| 14 | 示例统一架构 | runTest + main 入口、testXxx 能力点方法、退出码常量 |
| 15 | Lambda effectively final | Lambda 引用变量必须 final 或 effectively final |
| 16 | 示例 pom 同步 includes | 新增示例必须同步更新 maven-compiler-plugin includes |

---

## 规则详细说明

### 规则 1：中文详细注释

**[强制]** 所有注释（类、方法、属性、内部逻辑）必须使用中文书写，语义清晰完整。

**说明：** 专有名词与关键字保持英文，其余全部使用中文。

```java
/**
 * HTTP Server 综合示例 — 基于 Server SPI，支持全部实现切换与自检。
 *
 * <p>通过命令行参数指定 {@code @Spi} 类型，JDK HttpClient 自检覆盖基础能力矩阵。</p>
 *
 * <h2>用法</h2>
 * <pre>
 *   # 默认 jdk，端口 8080，常驻运行
 *   java HttpServerExample
 *
 *   # 指定实现类型 + 端口
 *   java HttpServerExample --type netty-http --port 9090
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class HttpServerExample {
```

---

### 规则 2：属性必须多行注释

**[强制]** 所有属性（字段、常量）必须使用 Javadoc 多行注释 `/** */`，禁止使用单行 `//` 注释属性。

```java
/**
 * 默认端口号
 */
private static final int DEFAULT_PORT = 8080;

/**
 * 测试超时时间（秒）
 */
private static final int TEST_TIMEOUT_SECONDS = 30;

/**
 * 是否启用 Reactor 模式
 */
private boolean reactor;

/**
 * API Key
 */
private String apiKey;
```

**反例：**
```java
private static final int DEFAULT_PORT = 8080;   // 默认端口
private boolean reactor;                       // 是否 Reactor
```

---

### 规则 3：单行注释必须在代码上方，不允许在后面

**[强制]** 方法内部单行注释 `//` 必须独占一行，位于被注释代码的上方，禁止放在代码行尾。

```java
// 保存全局配置
globalApiKey = parsed.apiKey;
globalBaseUrl = parsed.baseUrl;
globalModel = parsed.model;
```

**反例：**
```java
globalApiKey = parsed.apiKey;   // 保存全局配置
```

---

### 规则 4：类注释添加 @author CH

**[强制]** 所有类的 Javadoc 注释必须包含 `@author CH` 标签。

```java
/**
 * 对象池示例，通过 -Dpool.test=xxx 切换测试。
 *
 * <p>可选的测试：basicBorrowReturn、poolGuard、concurrentBorrow、invalidateObject、poolExhaustion</p>
 *
 * @author CH
 * @since 2024/12/12
 */
public class ObjectPoolExample {
```

---

### 规则 5：if/else/while 等必须使用大括号

**[强制]** 所有控制语句（if、else、for、while、do-while）必须使用大括号 `{}`，即使只有一行代码。

```java
if (server == null) {
    System.err.println("[FAIL] 无法创建 Server[type=" + type + "]");
    System.exit(1);
}

if (type.equals("jdk")) {
    return "jdk";
} else {
    return "unknown";
}
```

**反例：**
```java
if (server == null) System.exit(1);
if (type.equals("jdk")) return "jdk"; else return "unknown";
```

---

### 规则 6：代码不允许压缩成一行

**[强制]** 多行代码必须展开，禁止将多行代码压缩为一行。

**涵盖范围：**
- if/for/while 体与条件在同一行
- 方法调用链超过 120 字符时
- 多个语句用分号分隔在同一行

```java
if (collection != null && !collection.isEmpty()) {
    for (Object item : collection) {
        handle(item);
    }
}
```

**反例：**
```java
if(collection!=null&&!collection.isEmpty()){for(Object item:collection){handle(item);}}
```

---

### 规则 7：使用 Lombok 和 Record 简化代码和日志

**[强制]** 优先使用 Lombok 注解和 Java Record 简化代码：

- **日志**：使用 `@Slf4j`（SLF4J），禁止手动创建 Logger
- **参数容器**：内部静态类使用 `@Data` 或 `record`
- **不可变数据**：优先使用 Java `record`

```java
import lombok.extern.slf4j.Slf4j;

@Slf4j
public class DispatcherProviderExample {

    public static void main(String[] args) throws Exception {
        String providerName = System.getProperty("dispatcher.provider", "memory");
        log.info("测试分发器: {}", providerName);
    }
}

/**
 * 命令行参数容器（Record 形式）。
 *
 * @param type    Provider 类型
 * @param port    端口号
 * @param test    是否自检模式
 * @param reactor 是否启用 Reactor
 * @param help    是否打印帮助
 * @author CH
 * @since 4.0.0.42
 */
private record Args(
    String type,
    int port,
    boolean test,
    boolean reactor,
    boolean help
) {}
```

---

### 规则 8：删除 package 上方的注释

**[强制]** `package` 语句上方不得有任何注释（包括文件头注释、License 注释等）。
类注释必须写在 `package` 和 `import` 之后，`class` 声明之前。

```java
package com.chua.example.network.server;

// 此处不得有任何注释

import com.chua.common.support.network.server.Server;

/**
 * HTTP Server 综合示例。
 *
 * @author CH
 * @since 4.0.0.42
 */
public class HttpServerExample {
```

**反例：**
```java
// ==================== 文件头注释 ====================
// 版权所有 ...
// 创建者 ...

package com.chua.example.network.server;
```

---

### 规则 9：Lombok 和 Record 简化代码

**[强制]** 与规则 7 相同，再次强调。所有新代码必须使用 Lombok 或 Record。

- POJO/DTO/VO/参数容器 → `@Data` + `@NoArgsConstructor` + `@AllArgsConstructor`，或 `record`
- 日志 → `@Slf4j`
- Builder → `@Builder`

```java
@Data
@Builder
private static final class Args {
    /**
     * Provider 类型
     */
    private String type;

    /**
     * 端口号
     */
    private int port;
}
```

---

### 规则 10：Record 的注释直接注释在类上而不是字段上

**[强制]** 使用 `record` 时，注释必须写在 record 声明处（作为类的 Javadoc），不在各字段上单独注释。

```java
/**
 * 命令行参数容器。
 *
 * @param type    SPI 类型标识
 * @param port    监听端口
 * @param test    是否自检模式
 * @param reactor 是否启用 Reactor
 * @param ssl     是否启用 SSL/TLS
 * @param help    是否打印帮助
 * @author CH
 * @since 4.0.0.42
 */
private record Args(
    String type,
    int port,
    boolean test,
    boolean reactor,
    boolean ssl,
    boolean help
) {}
```

**反例：**
```java
private record Args(
    /** SPI 类型标识 */ String type,   // 错误：字段上注释
    /** 监听端口 */     int port       // 错误：字段上注释
) {}
```

---

### 规则 11：修复代码上的问题

**[强制]** 所有代码必须消除以下问题：

- **魔法值**：所有字符串/数字常量必须提取为 `private static final` 常量
- **NPE 风险**：字符串比较使用 `"常量".equals(变量)`；Optional/判空处理
- **资源泄漏**：流、连接等必须使用 try-with-resources
- **异常处理**：不捕获 `RuntimeException`；顶层必须处理异常
- **日志规范**：使用 `{}` 占位符，禁止 `+` 拼接

```java
private static final String ERROR_SERVER_CREATE = "[ERROR] 无法创建 Server[type={}]";

// 错误：魔法值
if (response.statusCode() == 200) { ... }

// 正确：提取常量
private static final int HTTP_STATUS_OK = 200;
if (response.statusCode() == HTTP_STATUS_OK) { ... }
```

---

### 规则 12：所有的例子必须写在 utils-support-example-starter

**[强制]** 所有 `XxxExample` 示例类必须写在 `utils-support-example-starter` 模块中。

**包名规范：** 与接口/实现所在包一致。

```text
utils-support-example-starter/
└── src/
    ├── main/java/com/chua/example/
    │   └── network/server/
    │       ├── HttpServerExample.java      ← 示例类
    │       └── 单元测试覆盖矩阵.md         ← 测试矩阵
    │   └── concurrent/dispatcher/
    │       ├── DispatcherProviderExample.java
    │       └── 单元测试覆盖矩阵.md
    │   └── ...
    └── test/java/com/chua/example/
        └── ...
```

**示例类命名规范：** `XxxExample.java`

**示例类结构要求：**
- 必须包含 `public static void main(String[] args)` 入口
- 必须通过 SPI 加载实现（`ServiceProvider.of(Xxx.class).getExtension(arg)`）
- 入参通过 `arg` 接收提供商和其他参数
- 必须包含自检/测试逻辑（`--test` 模式）
- 类注释添加 `@author CH`

**单元测试覆盖矩阵.md 必须包含：**

```markdown
# HttpServerExample 单元测试覆盖矩阵

## 版本信息
- 类名：HttpServerExample
- 模块：utils-support-example-starter
- 作者：CH
- 更新日期：YYYY-MM-DD

## SPI 实现覆盖矩阵

| 实现类型 | 基础路由 | 路径参数 | SSE | Reactor | SSL | 状态 |
|:--------|:--------|:--------|:----|:--------|:----|:----|
| jdk     | ✅      | ❌      | ✅  | ❌      | ❌  | 通过 |
| netty-http | ✅   | ✅      | ✅  | ✅      | ❌  | 通过 |

## 测试场景覆盖矩阵

| 场景ID | 测试方法 | 入参 | 断言 | 通过条件 |
|:------|:--------|:-----|:-----|:--------|
| TC-01 | testGetHello | baseUrl | 200 + Hello World | status==200 && body.equals("Hello World") |

## 执行记录
| 日期 | 实现类型 | 结果 | 备注 |
|:-----|:--------|:-----|:-----|
```

---

### 规则 13：必须遵守 P3C（阿里巴巴 Java 开发手册·黄山版）

**[强制]** 所有代码必须完整遵守《阿里巴巴 Java 开发手册（黄山版）》全部七大维度规约：

1. **编程规约**：命名风格、常量定义、代码格式、OOP、集合处理、并发处理、控制语句、注释规约、其他
2. **异常日志**：异常处理、日志规范
3. **单元测试**：AIR 原则、BCDE 原则
4. **安全规约**：权限控制、参数校验、SQL 注入防护
5. **MySQL 数据库**：建表、索引、SQL、ORM 映射
6. **工程结构**：应用分层、二方库依赖、服务器
7. **设计规约**：评审、用例图、状态图、时序图、类图、活动图、TDD

**重点关注项（P3C 强制级别）：**

- 所有命名不得使用拼音/中文
- 包名全部小写，点分隔符之间有且仅有一个自然语义单词
- 常量全部大写，单词间下划线隔开
- `if/for/while` 必须使用大括号
- 方法参数多个时，每个参数独立一行
- 链式调用每级调用独立一行
- 所有抽象方法必须加 Javadoc
- 类必须添加创建者和创建日期（`@author CH`）
- 方法内部单行注释在被注释语句上方
- 使用 SLF4J 占位符 `{}`，禁止 `+` 拼接
- try-with-resources 管理可关闭资源
- finally 块中不使用 return
- 集合初始化时指定初始容量
- 使用 entrySet 遍历 Map

---

### 规则 14：示例工程统一架构（runTest + main 入口）

**[强制]** 所有 `XxxExample` 示例类必须遵循统一的"自检+入口"架构：

- **`main(String[] args)`**：命令行入口，解析参数（如 `--type xxx` / `--help`），调用 `runTest`，最后 `System.exit` 返回退出码
- **`runTest(String type)`**：对外公开的自检入口，返回 `boolean`，便于单元测试调用
- **`testXxx()`**：每个能力点一个独立的 `public` 测试方法，返回 `boolean`，捕获所有异常并打印自检结果
- **常量提取**：所有魔法值（明文、密钥、超时、退出码、SPI 名称等）必须提取为 `private static final` 常量
- **退出码**：成功 `0`，失败 `1`，必须用命名常量而非裸数字

**架构示例：**

```java
public class CryptoExample {

    /**
     * 程序退出码：成功
     */
    private static final int EXIT_CODE_SUCCESS = 0;

    /**
     * 程序退出码：失败
     */
    private static final int EXIT_CODE_FAILURE = 1;

    public static void main(String[] args) {
        String type = parseType(args);
        CryptoExample example = new CryptoExample();
        boolean passed = example.runTest(type);
        System.exit(passed ? EXIT_CODE_SUCCESS : EXIT_CODE_FAILURE);
    }

    public boolean runTest(String type) {
        switch (type.toLowerCase()) {
            case "sm2" -> { return testSm2(); }
            case "all" -> { return testSm2() && testSm4(); }
            default -> { return false; }
        }
    }

    public boolean testSm2() {
        try {
            // ... 自检逻辑 ...
            return decryptOk && verified;
        } catch (Exception e) {
            System.err.println("[CryptoExample] SM2 failed: " + e.getMessage());
            return false;
        }
    }
}
```

---

### 规则 15：Lambda 引用变量必须为 effectively final

**[强制]** Lambda 表达式引用的外部局部变量必须为 final 或 effectively final（只赋值一次）。

**反例** — `pool` 先声明为 `null` 后赋值，Lambda 引用编译失败：

```java
ObjectPool<T> pool = null;
try {
    pool = createPool(...);
    executor.submit(() -> {
        T obj = pool.borrow();   // 编译错误：pool 不是 effectively final
    });
}
```

**正例** — 引入 `final` 别名：

```java
ObjectPool<T> pool = null;
try {
    pool = createPool(...);
    final ObjectPool<T> finalPool = pool;
    executor.submit(() -> {
        T obj = finalPool.borrow();   // OK
        finalPool.returnObject(obj);
    });
}
```

---

### 规则 16：示例 pom 必须同步更新 includes

**[强制]** 新增 `XxxExample.java` 后，必须同步更新 `utils-support-example-starter/pom.xml` 中的 `maven-compiler-plugin` `<includes>` 列表，否则示例不会被编译。

```xml
<plugin>
    <groupId>org.apache.maven.plugins</groupId>
    <artifactId>maven-compiler-plugin</artifactId>
    <configuration>
        <includes>
            <!-- 既有示例 -->
            <include>**/HttpServerExample.java</include>
            <!-- 新增示例 -->
            <include>**/CryptoExample.java</include>
            <include>**/DateTimeExample.java</include>
            <include>**/SerializerExample.java</include>
            <include>**/ObjectPoolExample.java</include>
        </includes>
    </configuration>
</plugin>
```

---

## 示例代码模板

所有示例类必须遵循以下模板：

```java
package com.chua.example.<module>.<submodule>;

import com.chua.common.support.spi.ServiceProvider;
import lombok.extern.slf4j.Slf4j;

import java.net.http.HttpClient;
import java.net.URI;

/**
 * Xxx 综合示例 — 基于 Xxx SPI，支持全部实现切换与自检。
 *
 * <p>通过命令行参数指定 {@code @Spi} 类型，自检覆盖基础能力矩阵。</p>
 *
 * <h2>用法</h2>
 * <pre>
 *   # 默认实现，端口 8080，常驻运行
 *   java XxxExample
 *
 *   # 指定实现类型 + 端口
 *   java XxxExample --type xxx-type --port 9090
 *
 *   # 自检模式：启动后发请求验证，通过后自动关闭
 *   java XxxExample --type xxx-type --test
 *
 *   # 打印帮助
 *   java XxxExample --help
 * </pre>
 *
 * <h2>SPI 类型与能力</h2>
 * <table border="1">
 *   <tr><th>--type</th><th>实现类</th><th>能力 A</th><th>能力 B</th></tr>
 *   <tr><td>default</td><td>DefaultXxx</td><td>✅</td><td>✅</td></tr>
 * </table>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public class XxxExample {

    /**
     * 默认端口号
     */
    private static final int DEFAULT_PORT = 8080;

    /**
     * 测试超时时间（秒）
     */
    private static final int TEST_TIMEOUT_SECONDS = 30;

    // ==================== main ====================

    public static void main(String[] args) {
        Args parsed = parseArgs(args);

        if (parsed.help()) {
            printHelp();
            return;
        }

        String implType = parsed.type() != null ? parsed.type() : "default";
        int port = parsed.port() > 0 ? parsed.port() : DEFAULT_PORT;

        if (parsed.test()) {
            runTest(implType, port);
            return;
        }

        runServer(implType, port);
    }

    // ==================== 常驻模式 ====================

    private static void runServer(String type, int port) {
        ServerSetting setting = ServerSetting.defaults();
        setting.setPort(port);

        ConfigServer server = createServer(type, setting);
        if (server == null) {
            System.err.println("[ERROR] 无法创建 Server[type=" + type + "]");
            System.exit(1);
        }

        registerRoutes(server, type);
        server.start();

        System.out.println("[XXX] " + type + " 服务就绪: http://localhost:" + port);
        awaitShutdown();
    }

    /**
     * 通过 SPI 创建 Server 实例。
     *
     * @param type    SPI 类型标识
     * @param setting 服务器配置
     * @return ConfigServer 实例，创建失败返回 null
     */
    private static ConfigServer createServer(String type, ServerSetting setting) {
        try {
            Server raw = ServiceProvider.of(Server.class).getNewExtension(type, setting);
            if (raw == null) {
                System.err.println("[ERROR] 未找到 Server 实现: " + type);
                return null;
            }
            if (!(raw instanceof ConfigServer configServer)) {
                System.err.println("[ERROR] 当前实现不支持 HTTP 路由: " + raw.getClass().getName());
                return null;
            }
            return configServer;
        } catch (NoClassDefFoundError | ExceptionInInitializerError e) {
            System.err.println("[WARN] Server[" + type + "] 依赖缺失，回退到 default: " + e.getMessage());
            try {
                Server fallback = ServiceProvider.of(Server.class).getNewExtension("default", setting);
                if (fallback instanceof ConfigServer configServer) {
                    return configServer;
                }
            } catch (Exception ignored) {
                // default 回退也失败
            }
            return null;
        }
    }

    /**
     * 注册演示路由。
     *
     * @param server HTTP 服务器实例
     * @param type   实现类型标识
     */
    private static void registerRoutes(ConfigServer server, String type) {
        server.registerMapping("/hello", HttpMethod.GET, (request, response) -> {
            response.setResult("Hello World");
        });

        server.registerMapping("/json", HttpMethod.GET, (request, response) -> {
            response.setBody("{\"message\":\"hello\",\"type\":\"" + type + "\"}");
            response.setContentType(HttpHeader.APPLICATION_JSON);
        });
    }

    private static void awaitShutdown() {
        try {
            new CountDownLatch(1).await();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    // ==================== 自检模式 ====================

    private static void runTest(String type, int port) {
        log.info("===== XxxExample --test [type={}, port={}] =====", type, port);

        ServerSetting setting = ServerSetting.defaults();
        setting.setPort(port);

        ConfigServer server = createServer(type, setting);
        if (server == null) {
            System.err.println("[FAIL] 无法创建 Server[type=" + type + "]");
            System.exit(1);
        }

        registerRoutes(server, type);
        server.start();

        HttpClient client = HttpClient.newHttpClient();
        String baseUrl = "http://127.0.0.1:" + port;

        try {
            Thread.sleep(500);
            boolean allPassed = true;
            allPassed &= testGetHello(client, baseUrl);
            allPassed &= testGetJson(client, baseUrl, type);
            allPassed &= testNotFound(client, baseUrl);

            System.out.println("-----");
            if (allPassed) {
                System.out.println("[PASS] " + type + " 全部自检通过");
            } else {
                System.out.println("[FAIL] " + type + " 存在失败的测试项");
                System.exit(1);
            }
        } catch (Exception e) {
            System.err.println("[ERROR] 测试异常: " + e.getMessage());
            System.exit(1);
        } finally {
            server.stop();
            client.close();
        }
    }

    private static boolean testGetHello(HttpClient client, String baseUrl) throws Exception {
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + "/hello"))
                .GET()
                .build();
        HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());
        boolean passed = response.statusCode() == HTTP_STATUS_OK
                && "Hello World".equals(response.body());
        printResult("GET /hello -> 200 + Hello World", passed);
        return passed;
    }

    private static boolean testGetJson(HttpClient client, String baseUrl, String type) throws Exception {
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + "/json"))
                .GET()
                .build();
        HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());
        boolean passed = response.statusCode() == HTTP_STATUS_OK
                && response.body().contains("\"type\":\"" + type + "\"");
        printResult("GET /json -> 200 + JSON body", passed);
        return passed;
    }

    private static boolean testNotFound(HttpClient client, String baseUrl) throws Exception {
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + "/not-exist-route-404"))
                .GET()
                .build();
        HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());
        boolean passed = response.statusCode() == HTTP_STATUS_NOT_FOUND;
        printResult("GET /not-exist -> 404", passed);
        return passed;
    }

    private static void printResult(String name, boolean passed) {
        System.out.println((passed ? "[PASS]" : "[FAIL]") + " " + name);
    }

    // ==================== 参数解析 ====================

    /**
     * HTTP 状态码：200 OK
     */
    private static final int HTTP_STATUS_OK = 200;

    /**
     * HTTP 状态码：404 Not Found
     */
    private static final int HTTP_STATUS_NOT_FOUND = 404;

    /**
     * 解析命令行参数。
     *
     * @param args 命令行参数数组
     * @return 参数对象
     */
    private static Args parseArgs(String[] args) {
        Args result = new Args();
        int index = 0;
        while (index < args.length) {
            switch (args[index]) {
                case "--type", "-t" -> {
                    if (index + 1 < args.length) {
                        result = result.withType(args[++index]);
                    }
                }
                case "--port", "-p" -> {
                    if (index + 1 < args.length) {
                        result = result.withPort(Integer.parseInt(args[++index]));
                    }
                }
                case "--test" -> result = result.withTest(true);
                case "--reactor" -> result = result.withReactor(true);
                case "--ssl" -> result = result.withSsl(true);
                case "--help", "-h" -> result = result.withHelp(true);
                default -> System.err.println("[WARN] 未知参数: " + args[index]);
            }
            index++;
        }
        return result;
    }

    /**
     * 打印帮助信息。
     */
    private static void printHelp() {
        System.out.println("Xxx 综合示例 — 基于 Xxx SPI");
        System.out.println();
        System.out.println("用法: java XxxExample [选项]");
        System.out.println();
        System.out.println("选项:");
        System.out.println("  --type,    -t <key>    实现类型（默认: default）");
        System.out.println("  --port,    -p <port>    监听端口（默认: 8080）");
        System.out.println("  --test                 启动后运行自检并退出");
        System.out.println("  --reactor              启用 Reactor 模式");
        System.out.println("  --ssl                  启用 SSL/TLS");
        System.out.println("  --help,  -h             显示此帮助");
    }

    // ==================== 参数容器 ====================

    /**
     * 命令行参数容器。
     *
     * @param type    SPI 类型标识
     * @param port    监听端口
     * @param test    是否自检模式
     * @param reactor 是否启用 Reactor
     * @param ssl     是否启用 SSL/TLS
     * @param help    是否打印帮助
     * @author CH
     * @since 4.0.0.42
     */
    private record Args(
        String type,
        int port,
        boolean test,
        boolean reactor,
        boolean ssl,
        boolean help
    ) {
        /**
         * 带默认值的空参构造。
         */
        Args() {
            this(null, 0, false, false, false, false);
        }

        /**
         * 替换 type 字段，返回新实例。
         *
         * @param type SPI 类型标识
         * @return 新 Args 实例
         */
        public Args withType(String type) {
            return new Args(type, port, test, reactor, ssl, help);
        }

        /**
         * 替换 port 字段，返回新实例。
         *
         * @param port 监听端口
         * @return 新 Args 实例
         */
        public Args withPort(int port) {
            return new Args(type, port, test, reactor, ssl, help);
        }

        /**
         * 替换 test 字段，返回新实例。
         *
         * @param test 是否自检
         * @return 新 Args 实例
         */
        public Args withTest(boolean test) {
            return new Args(type, port, test, reactor, ssl, help);
        }

        /**
         * 替换 reactor 字段，返回新实例。
         *
         * @param reactor 是否 Reactor
         * @return 新 Args 实例
         */
        public Args withReactor(boolean reactor) {
            return new Args(type, port, test, reactor, ssl, help);
        }

        /**
         * 替换 ssl 字段，返回新实例。
         *
         * @param ssl 是否 SSL
         * @return 新 Args 实例
         */
        public Args withSsl(boolean ssl) {
            return new Args(type, port, test, reactor, ssl, help);
        }

        /**
         * 替换 help 字段，返回新实例。
         *
         * @param help 是否帮助
         * @return 新 Args 实例
         */
        public Args withHelp(boolean help) {
            return new Args(type, port, test, reactor, ssl, help);
        }
    }
}
```

---

## P3C 补充检查清单

在提交代码前，必须逐项确认以下 P3C 强制规则：

### 编程规约
- [ ] 命名不使用拼音/中文/下划线开头或结尾
- [ ] 类名 UpperCamelCase，方法/字段 lowerCamelCase
- [ ] 常量全部大写，单词间下划线
- [ ] POJO 布尔不加 `is` 前缀
- [ ] 数组声明 `String[] args` 而非 `String args[]`
- [ ] 魔法值已提取为常量
- [ ] `long`/`Long` 使用大写 `L` 后缀

### 代码格式
- [ ] 左大括号不换行，右大括号后换行
- [ ] 空格：`if (xxx)` 保留字与括号间有空格
- [ ] 运算符左右有空格：`a + b`
- [ ] 4 空格缩进，无 tab
- [ ] 单行字符不超过 120
- [ ] 方法参数多个时逗号后有空格

### OOP 规约
- [ ] 覆写方法加 `@Override`
- [ ] 包装类 equals 比较，不使用 `==`
- [ ] POJO 属性使用包装类型
- [ ] POJO 写 toString（Lombok @Data 自动生成）
- [ ] 静态方法/变量使用类名访问

### 集合处理
- [ ] 重写 equals 同时重写 hashCode
- [ ] 使用 entrySet 遍历 Map
- [ ] 集合初始化指定初始容量
- [ ] 不在 foreach 中 remove/add

### 并发处理
- [ ] 线程池使用 ThreadPoolExecutor 而非 Executors
- [ ] SimpleDateFormat 使用 ThreadLocal 或 DateTimeFormatter
- [ ] CountDownLatch 在 finally/catch 中 countDown

### 控制语句
- [ ] switch 有 default
- [ ] if/else/for/while/do 必须大括号
- [ ] 超过 3 层 if-else 改用卫语句/策略模式

### 注释规约
- [ ] 类/方法/属性使用 Javadoc `/** */`
- [ ] 单行注释 `//` 在代码上方
- [ ] 所有类包含 `@author CH` 和 `@since`
- [ ] 抽象方法有完整 Javadoc（参数、返回值、异常）

### 异常日志
- [ ] 不捕获 RuntimeException
- [ ] 不使用异常做流程控制
- [ ] 使用 SLF4J 占位符 `{}`
- [ ] try-with-resources 管理资源
- [ ] finally 不使用 return

### 安全规约
- [ ] 用户敏感数据脱敏
- [ ] SQL 参数绑定
- [ ] 禁止 HTML 输出未转义用户数据

### 工程结构
- [ ] 包名小写，单数形式
- [ ] 依赖方向正确（不反向依赖）
- [ ] 不在子项目 pom 中使用 SNAPSHOT

---

## 快速参考卡

### ✅ 推荐做法

```java
package com.chua.example.network.server;

import lombok.extern.slf4j.Slf4j;

/**
 * HTTP Server 综合示例。
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public class HttpServerExample {

    /**
     * 默认端口号
     */
    private static final int DEFAULT_PORT = 8080;

    public static void main(String[] args) {
        Args parsed = parseArgs(args);

        if (parsed.help()) {
            printHelp();
            return;
        }

        // 解析实现类型
        String type = parsed.type() != null ? parsed.type() : "default";
        int port = parsed.port() > 0 ? parsed.port() : DEFAULT_PORT;

        if (parsed.test()) {
            runTest(type, port);
        } else {
            runServer(type, port);
        }
    }
}

/**
 * 命令行参数容器。
 *
 * @param type    SPI 类型标识
 * @param port    监听端口
 * @param test    是否自检模式
 * @author CH
 * @since 4.0.0.42
 */
private record Args(String type, int port, boolean test) {
    Args() {
        this(null, 0, false);
    }
}
```

### ❌ 禁止做法

```java
// package 上方有注释 — 禁止
// ==================== 文件头 ====================
package com.chua.example;

import lombok.extern.slf4j.Slf4j;

// 类无 @author CH — 禁止
public class BadExample {
    private int port = 8080;                    // 行尾注释 — 禁止
    private String type;                        // 无多行注释 — 禁止

    public static void main(String[] args) {
        if (args.length > 0) type = args[0];    // 无大括号 — 禁止
        System.out.println("start");            // 魔法值 — 禁止
    }
}
```
