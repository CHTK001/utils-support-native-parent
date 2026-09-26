# utils-support-native-needle

Needle 推理引擎的 Java 门面。引擎本体是 cactus-compute 官方发布的二进制
（[cactus-compute/needle](https://github.com/cactus-compute/needle)），
**不随本仓库分发**，需自行获取后按下述方式提供。

本模块只做 FFM 绑定，不含任何模型逻辑。

## 部署

推理需要**两样东西同时就位**，缺一不可：

| # | 文件 | 获取方式 |
|---|------|----------|
| 1 | 共享库 `libneedle3.dll` / `.so` / `.dylib` | 见下方「共享库怎么拿」 |
| 2 | 权重归档 `needle3.cact`（约 34 MB） | `huggingface.co/Cactus-Compute/needle3` 的 `needle3.cact` |

把两个文件放同一目录后，只需配一个参数：

```
-Dchua.needle.native.dir=<目录>          # 或环境变量 CHUA_NEEDLE_NATIVE_DIR
```

权重未显式配置时会在**引擎库同目录**自动查找 `*.cact`，故同目录布局下
`chua.needle.weights` 可省略。也可显式指定：

```
-Dchua.needle.weights=<文件路径>        # 或环境变量 CHUA_NEEDLE_WEIGHTS
```

### 共享库怎么拿（容易踩坑）

HuggingFace 仓库里按平台分发的文件**大多不能用**，别下错：

| 路径 | 内容 | 能否给 FFM 用 |
|------|------|----------------|
| `windows-x86_64/needle.exe` | CLI 运行器，`Subsystem=Windows CUI`，**无导出表** | ✗ |
| `linux-x86_64/needle` | 同上，且**无扩展名** | ✗ |
| `<platform>/libneedle.a` | 静态库 | ✗ 需自行编译 |
| `python/cactus_needle-*-py3-none-<平台标签>.whl` 内的 `needle/libneedle3.dll` | **共享库** | ✓ |

最省事的做法是让官方 Python 包替你下载并缓存：

```sh
pip install cactus-needle
python -c "import needle; needle.Needle()"   # 首次使用会拉取引擎
```

然后把 `chua.needle.native.dir` 指向
`~/.cache/cactus-needle/v3/<版本>/`（Windows 为
`%USERPROFILE%\.cache\cactus-needle\v3\<版本>\`），该目录里同时有
`libneedle3.dll` 与 `needle3.cact`。

也可直接从 wheel 抽取：下载对应平台标签的 `.whl`，取出其中的
`needle/libneedle3.dll`。

若把目录误配成只有 CLI 运行器的位置，加载器会明确提示这一点，而不是报
"缺少符号"。

### 代际不可混用

Needle 2 与 Needle 3 是两个不同的库文件（`libneedle2.*` / `libneedle3.*`），
ABI 不同。本模块按 **Needle 3** 绑定；同目录下两代并存时优先选 3。

### 运行时要求

FFM（`java.lang.foreign`）自 JDK 22 转正，本模块 `maven.compiler.release=22`，
**运行时需 JDK 22+**（父 POM 未统一配置编译级别，本模块单独提升）。

`SymbolLookup.libraryLookup`、`Linker.nativeLinker`、`MemorySegment.reinterpret`
均为 JDK 受限方法，JDK 24 起默认打印告警。需要静默时加：

```
--enable-native-access=ALL-UNNAMED
```

## 用法

```java
// 库与权重就绪后
NeedleNative.init("你是家庭助手", toolsJson, null);
String envelope = NeedleNative.complete("turn off the bedroom lights", 512);
```

部署前先判断，避免缺件时抛异常：

```java
if (!NeedleNative.isLoaded()) {
    log.warn("needle 引擎未就绪: {}", NeedleNative.getLoadError().getMessage());
    return;
}
```

`isLoaded()` 只代表库与符号就绪；`isReady()` 进一步要求权重也已绑定。

完整 API：`init` / `complete` / `embed` / `reset`，详见 `NeedleNative`。

## 线程模型（重要）

官方头文件第一句就是 `One process-global, non-thread-safe model.`，且
**权重一旦绑定就无法卸载**（绑定 tuned 权重后再调用 base 模型 agent 会直接抛错）。
因此：

- `NeedleEngine` 用一把全局锁把 `init` / `complete` / `embed` / `reset`
  **全部串行化**，多线程并发会排队而非并行；
- `needle_load` 每个 JVM 进程**至多成功一次**；
- `init` 在 system 与 tools 未变时**幂等**，可每请求调用一次。

适合"单会话串行"用法，**不适合需要并发吞吐的场景**。真要并发，需在应用层
做多会话路由并隔离到不同进程。

## 多轮状态污染（重要）

引擎的 `complete` 会**延续同一会话上下文**，且倾向把新指令当成上一轮的后续。
实测中连续下达不同指令时，它继承了历史参数：

```
第 1 轮  把客厅灯调到 30  →  room=客厅, brightness=30   ✓
第 2 轮  关掉卧室的灯      →  room=客厅, brightness=30   ✗ 沿用历史
第 3 轮  打开厨房的灯      →  room=客厅, brightness=30   ✗ 沿用历史
```

调用 `reset()` 后同一批指令结果正确。**若每条输入都是独立命令，
必须在调用 `complete` 前 `reset()`**，否则会串味。

`init` 是幂等的，**不会**顺带清历史——`NeedleChatClient#chatSync` 目前
每请求只调 `init` + `complete`、从不 `reset`，接入前需补上。

## ABI

绑定目标为官方 `needle.h` 声明的 6 个符号：

```c
int         needle_init(const char* system_prompt, const char* tools_json,
                        const char* tool_index_path);   /* 成功返回静态前缀 token 数 */
const char* needle_last_error(void);                    /* 下次 API 调用前有效 */
int         needle_complete(const char* input, int max_new_tokens,
                            char* out, int out_capacity);
int         needle_embed(const char* input, float* out, int out_capacity);
void        needle_reset(void);
int         needle_load(const unsigned char* cact, unsigned long long n);
```

约定与易错点：

- 返回 `< 0` 表示失败。`needle_init` 失败的具体原因**只能**经
  `needle_last_error` 取得（输出缓冲区不参与）；`needle_complete` 失败时
  原因既可能写进 `out`，也可能只在 `needle_last_error`。
  本模块两条路径都读，优先取 `out`。
- `needle_last_error` 返回**无长度的 C 字符串指针**，FFM 拿到的段
  `byteSize` 为 0，直接 `getString(0)` 会抛
  `IndexOutOfBoundsException: No null terminator found`。
  必须先 `reinterpret` 出有界窗口再读字符串。
- `needle_complete` 的输出缓冲区**由调用方提供**，默认 65536 字节，
  可用 `-Dchua.needle.buffer.size` 调整。
- `needle_embed` 传 `(NULL, 0)` 查询维度，再传一次填向量，仅 Needle 3 支持。
- `needle_load` 收的是 `.cact` 归档**字节**而非路径，本模块直接以
  `MemorySegment` 传入，无需落临时文件。

## 实测结论（真实引擎，Windows x86-64）

绑定本身**已用真实引擎验证**：中英文 prompt 与 system 正确传入、返回
JSON envelope、`embed` 得到 3072 维向量、`reset` 生效、`needle_last_error`
在 `needle_init` 上下文超限时给出真实原因。

但**模型侧有两点必须知悉**：

1. **中文能力明显弱于英文。** 官方 README 自己也承认
   *"The shipped base model does not pass all six suites... five fall short,
   missing calls the query did state and inventing values it did not."*
   实测同一工具集下：

   | 输入 | 结果 |
   |------|------|
   | `turn off the bedroom lights` | `room=bedroom, on=false`，confidence 0.98 ✓ |
   | `把客厅灯调到 30` | `room=把客厅灯`（把动词吞进房间名） |
   | `关掉卧室的灯` | 拒绝，`reasoning: "Query asks for a specific time"`（幻觉） |
   | `打开厨房的灯` | 拒绝，`reasoning: "Query asks for a calculation"`（幻觉） |

   模型是英文中心的。中文场景需自行评估，或改用英文 prompt / 微调
   （`needle finetune` + `needle build`）。

2. **CPU 上解码很慢。** 实测 `decode_tps` 约 0.9–2.1，单次调用 20–75 秒，
   峰值内存约 198 MB。官方标的 300–1500 tps 是移动端数字。不适合
   延迟敏感场景。
