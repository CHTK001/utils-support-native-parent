# utils-support-native-needle

Needle 推理引擎的 Java 门面。引擎本体是 cactus-compute 官方发布的 C 二进制
（[cactus-compute/needle](https://github.com/cactus-compute/needle)，
权重 [Cactus-Compute/needle3](https://huggingface.co/Cactus-Compute/needle3)），
**不随本仓库分发**，需自行下载后按下方方式提供。

本模块只做 FFM 绑定，不含任何模型逻辑。

## 部署

推理需要**两样东西同时就位**，缺一不可：

| # | 文件 | 来源 | 指定方式 |
|---|------|------|----------|
| 1 | 引擎库 `libneedle3.dll` / `.so` / `.dylib` | HuggingFace 引擎仓库 | `-Dchua.needle.native.dir=<目录>` 或 `CHUA_NEEDLE_NATIVE_DIR` |
| 2 | 权重归档 `*.cact` | HuggingFace `Cactus-Compute/needle3` | `-Dchua.needle.weights=<文件>` 或 `CHUA_NEEDLE_WEIGHTS` |

权重未显式配置时，会在**引擎库同目录**自动查找 `*.cact`（与官方缓存布局一致）。
把两个文件放同一目录时，只需配 `chua.needle.native.dir` 一个参数。

也可把引擎库打进 jar 放在 `native/<平台目录>/` 下，由 classpath 抽取加载。

### 代际不可混用

Needle 2 与 Needle 3 是**两个不同的库文件**（`libneedle2.*` / `libneedle3.*`），
ABI 不同。本模块按 **Needle 3** 绑定。同一目录下同时存在两代时优先选 3。
若误加载 Needle 2 库，会因缺少必需符号而报明确错误，不会静默降级。

### 运行时要求

FFM（`java.lang.foreign`）自 JDK 22 转正，本模块 `maven.compiler.release=22`，
**运行时需 JDK 22+**（父 POM 未统一配置编译级别，本模块单独提升）。

`SymbolLookup.libraryLookup` 与 `Linker.nativeLinker` 是 JDK 受限方法，
JDK 24 起默认打印告警。需要静默时加：

```
--enable-native-access=ALL-UNNAMED
```

## 用法

```java
// 库与权重就绪后
NeedleNative.init("你是家庭自动化助手", toolsJson, null);
String envelope = NeedleNative.complete("把客厅灯调到 30", 512);
```

部署前建议先判断，避免在缺件时抛异常：

```java
if (!NeedleNative.isLoaded()) {
    log.warn("needle 引擎未就绪: {}", NeedleNative.getLoadError().getMessage());
    return;
}
```

`isReady()` 进一步要求权重也已绑定；`isLoaded()` 只代表库与符号就绪。

完整 API：`init` / `complete` / `embed` / `reset`，详见 `NeedleNative`。

## 线程模型（重要）

引擎是**进程级单例，没有句柄**，且**权重一旦绑定就无法卸载**——官方明确说明，
绑定 tuned 权重后再构造或调用 base 模型 agent 会直接抛错。因此：

- `NeedleEngine` 用一把全局锁把 `init` / `complete` / `embed` / `reset`
  **全部串行化**，多线程并发会排队而非并行；
- `needle_load` 每个 JVM 进程**至多成功一次**；
- `init` 在 system 与 tools 未变时**幂等**，可以每请求调用一次
  （`NeedleChatClient#chatSync` 正是这么用的）。

这意味着本模块适合"单会话串行"用法，**不适合需要并发吞吐的场景**。
真要并发，需在应用层做多会话路由并隔离到不同进程。

## ABI

绑定目标为官方 Python 包 `cactus-needle` 的 ctypes 绑定所声明的 5 个符号：

```c
int  needle_init(const char* system, const char* tools_json, const char* tool_index_path);
int  needle_complete(const char* text, int max_new_tokens, char* out_buf, int out_buf_len);
int  needle_embed(const char* text, float* out, int dim);
void needle_reset(void);
int  needle_load(const char* data, uint64_t len);
```

约定：

- 返回 `< 0` 表示失败；`needle_complete` 失败时会把原因写进 `out_buf`，
  本模块会把它带进异常消息；
- `needle_complete` 的输出缓冲区**由调用方提供**，默认 65536 字节，
  可用 `-Dchua.needle.buffer.size` 调整；
- `needle_embed` 传 `(NULL, 0)` 查询维度，再传一次填向量，仅 Needle 3 支持；
- `needle_load` 收的是 `.cact` 归档**字节**而非路径，本模块直接以
  `MemorySegment` 传入，无需落临时文件。

## 当前状态

FFM 绑定已实现并通过桩库验证（导出同样 5 个符号、按同样 ABI 编译，
逐项断言字符串传递、缓冲区回写、权重加载、会话重置、嵌入维度与数值、
`init` 幂等、错误码透传，共 17 项）。

**尚未在真实引擎上验证**：本仓库拿不到 HuggingFace 的引擎与权重，
以上结论基于桩库。首次接入真实引擎时，仍需确认官方 ABI 未发生变更。
