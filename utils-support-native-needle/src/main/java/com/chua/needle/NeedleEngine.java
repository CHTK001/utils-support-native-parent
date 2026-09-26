package com.chua.needle;

import java.io.IOException;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.locks.ReentrantLock;
import java.util.stream.Stream;

/**
 * Needle 推理引擎的 FFM 绑定与会话管理。
 *
 * <p>绑定目标为 cactus-compute 发布的 C 推理引擎（{@code libneedle3.dll} /
 * {@code libneedle3.so} / {@code libneedle3.dylib}）。ABI 取自官方 Python 包
 * {@code cactus-needle} 的 ctypes 绑定，共 5 个导出符号：</p>
 *
 * <pre>{@code
 * int  needle_init(const char* system, const char* tools_json, const char* tool_index_path);
 * int  needle_complete(const char* text, int max_new_tokens, char* out_buf, int out_buf_len);
 * int  needle_embed(const char* text, float* out, int dim);
 * void needle_reset(void);
 * int  needle_load(const char* data, uint64_t len);
 * }</pre>
 *
 * <p><b>线程模型：全局单例，所有调用串行化。</b>引擎内部没有句柄，状态是进程级
 * 单例；且权重一旦绑定就无法卸载（官方原文：绑定 tuned 权重后再调用 base 模型
 * 会直接抛错）。因此本类用一把全局锁把 {@code init} / {@code complete} /
 * {@code embed} / {@code reset} 全部串行化，并保证 {@code needle_load} 至多成功一次。
 * 调用方无需自行加锁，但也<b>不应期待并发吞吐</b>。</p>
 *
 * <p><b>幂等性：</b>{@code init} 在 system 与 tools 与当前绑定一致时直接跳过，
 * 避免 {@code NeedleChatClient} 每次请求都重建会话。</p>
 *
 * <p><b>原生访问：</b>{@code SymbolLookup#libraryLookup} 与
 * {@code Linker#nativeLinker} 是 JDK 受限方法，JDK 24 起默认打印告警。
 * 需要静默时给 JVM 加 {@code --enable-native-access=ALL-UNNAMED}。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
final class NeedleEngine {

    /**
     * 权重归档（{@code .cact}）路径所用的系统属性名
     */
    private static final String PROP_WEIGHTS = "chua.needle.weights";

    /**
     * 权重归档路径所用的环境变量名
     */
    private static final String ENV_WEIGHTS = "CHUA_NEEDLE_WEIGHTS";

    /**
     * 输出缓冲区大小所用的系统属性名
     */
    private static final String PROP_BUFFER_SIZE = "chua.needle.buffer.size";

    /**
     * 输出缓冲区大小所用的环境变量名
     */
    private static final String ENV_BUFFER_SIZE = "CHUA_NEEDLE_BUFFER_SIZE";

    /**
     * 输出缓冲区默认大小，与官方 Python 绑定一致
     */
    private static final int DEFAULT_BUFFER_SIZE = 65536;

    /**
     * 引擎锁。引擎为进程级单例且不可卸载权重，故所有原生调用必须串行。
     */
    private static final ReentrantLock ENGINE_LOCK = new ReentrantLock();

    /**
     * {@code needle_init} 绑定
     */
    private static volatile MethodHandle needleInit;

    /**
     * {@code needle_complete} 绑定
     */
    private static volatile MethodHandle needleComplete;

    /**
     * {@code needle_embed} 绑定；引擎为 Needle 2 时不存在，此时为 null
     */
    private static volatile MethodHandle needleEmbed;

    /**
     * {@code needle_reset} 绑定
     */
    private static volatile MethodHandle needleReset;

    /**
     * {@code needle_load} 绑定
     */
    private static volatile MethodHandle needleLoad;

    /**
     * 权重是否已通过 {@code needle_load} 成功绑定
     */
    private static volatile boolean weightsLoaded;

    /**
     * 当前会话是否已通过 {@code needle_init} 建立
     */
    private static volatile boolean sessionReady;

    /**
     * 当前会话绑定的 system 文本
     */
    private static volatile String boundSystem = "";

    /**
     * 当前会话绑定的 tools JSON
     */
    private static volatile String boundTools = "";

    /**
     * 引擎库所在目录，用于在未显式配置时发现同目录的 {@code .cact} 权重
     */
    private static volatile Path libraryDir;

    private NeedleEngine() {
    }

    /**
     * 解析并缓存 5 个原生符号。
     *
     * <p>必须在动态库加载之后调用一次。{@code needle_embed} 允许缺失
     * （Needle 2 引擎无此能力），其余符号缺失视为绑定失败。</p>
     *
     * @param lookup 已加载引擎库的符号查找器
     * @throws IllegalStateException 必需符号缺失时抛出
     */
    static void bind(SymbolLookup lookup) {
        Linker linker = Linker.nativeLinker();
        needleInit = downcall(linker, lookup, "needle_init",
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        needleComplete = downcall(linker, lookup, "needle_complete",
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
        needleReset = downcall(linker, lookup, "needle_reset",
                FunctionDescriptor.ofVoid());
        needleLoad = downcall(linker, lookup, "needle_load",
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));
        needleEmbed = lookup.find("needle_embed")
                .map(address -> linker.downcallHandle(address, FunctionDescriptor.of(
                        ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_INT)))
                .orElse(null);
    }

    /**
     * 解析单个必需符号。
     *
     * @param linker     链接器
     * @param lookup     符号查找器
     * @param name       符号名
     * @param descriptor 函数描述符
     * @return 下调方法句柄
     * @throws IllegalStateException 符号不存在时抛出
     */
    private static MethodHandle downcall(Linker linker, SymbolLookup lookup,
                                         String name, FunctionDescriptor descriptor) {
        return linker.downcallHandle(lookup.find(name).orElseThrow(
                () -> new IllegalStateException("引擎缺少必需符号 " + name
                        + "；该动态库可能不是 cactus-compute 的 needle 引擎，"
                        + "或版本不匹配（Needle 2 与 Needle 3 是两个不同的库文件）")),
                descriptor);
    }

    /**
     * 记录引擎库所在目录，供未配置权重时在同目录发现 {@code .cact}。
     *
     * @param path 已加载的引擎库路径
     */
    static void libraryLocated(Path path) {
        Path parent = path.toAbsolutePath().getParent();
        if (parent != null) {
            libraryDir = parent;
        }
    }

    /**
     * 引擎是否已具备调用条件（符号已解析且权重已绑定）。
     *
     * @return 就绪返回 true
     */
    static boolean isReady() {
        return needleComplete != null && weightsLoaded && sessionReady;
    }

    /**
     * 权重是否已绑定。
     *
     * @return 已绑定返回 true
     */
    static boolean hasWeights() {
        return weightsLoaded;
    }

    /**
     * 建立或复用会话。
     *
     * <p>当 system 与 tools 与当前绑定完全一致时直接返回，不重复调用引擎。</p>
     *
     * @param system 系统提示词，可为 null
     * @param tools  工具声明 JSON，可为 null
     * @throws IllegalStateException 引擎未加载、权重缺失或引擎返回错误时抛出
     */
    static void init(String system, String tools) {
        String nextSystem = system == null ? "" : system;
        String nextTools = tools == null ? "[]" : tools;
        ENGINE_LOCK.lock();
        try {
            if (sessionReady && nextSystem.equals(boundSystem) && nextTools.equals(boundTools)) {
                return;
            }
            ensureWeights();
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment systemSeg = nextSystem.isEmpty()
                        ? MemorySegment.NULL : arena.allocateFrom(nextSystem);
                MemorySegment toolsSeg = nextTools.isEmpty()
                        ? MemorySegment.NULL : arena.allocateFrom(nextTools);
                int rc = (int) needleInit.invokeExact(systemSeg, toolsSeg, MemorySegment.NULL);
                if (rc < 0) {
                    throw new IllegalStateException(
                            "needle_init 失败，错误码 " + rc + "；请检查 tools 是否为合法 JSON 数组");
                }
            }
            boundSystem = nextSystem;
            boundTools = nextTools;
            sessionReady = true;
        } catch (IllegalStateException e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("needle_init 调用异常：" + t, t);
        } finally {
            ENGINE_LOCK.unlock();
        }
    }

    /**
     * 执行一次生成。
     *
     * @param prompt    用户提示文本
     * @param maxTokens 最大生成令牌数
     * @return 引擎返回的 JSON envelope 原文
     * @throws IllegalStateException 未建立会话或引擎返回错误时抛出
     */
    static String complete(String prompt, int maxTokens) {
        ENGINE_LOCK.lock();
        try {
            if (!sessionReady) {
                throw new IllegalStateException("会话尚未建立，请先调用 init()");
            }
            int bufferSize = bufferSize();
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment textSeg = prompt == null || prompt.isEmpty()
                        ? MemorySegment.NULL : arena.allocateFrom(prompt);
                // 多分配 1 字节并全 0，确保即使引擎写满也不越界读
                MemorySegment out = arena.allocate((long) bufferSize + 1L);
                int rc = (int) needleComplete.invokeExact(textSeg, maxTokens, out, bufferSize);
                if (rc < 0) {
                    throw new IllegalStateException("needle_complete 失败，错误码 " + rc
                            + "：" + out.getString(0));
                }
                return out.getString(0);
            }
        } catch (IllegalStateException e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("needle_complete 调用异常：" + t, t);
        } finally {
            ENGINE_LOCK.unlock();
        }
    }

    /**
     * 计算文本嵌入向量，仅 Needle 3 引擎支持。
     *
     * @param text 输入文本
     * @return 浮点向量
     * @throws UnsupportedOperationException 引擎无嵌入能力时抛出
     * @throws IllegalStateException           引擎返回错误时抛出
     */
    static float[] embed(String text) {
        ENGINE_LOCK.lock();
        try {
            MethodHandle embed = needleEmbed;
            if (embed == null) {
                throw new UnsupportedOperationException(
                        "当前引擎无 needle_embed 符号，嵌入能力需要 Needle 3 引擎");
            }
            if (!sessionReady) {
                throw new IllegalStateException("会话尚未建立，请先调用 init()");
            }
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment textSeg = text == null || text.isEmpty()
                        ? MemorySegment.NULL : arena.allocateFrom(text);
                int dim = (int) embed.invokeExact(textSeg, MemorySegment.NULL, 0);
                if (dim <= 0) {
                    throw new IllegalStateException("needle_embed 查询维度失败，错误码 " + dim);
                }
                MemorySegment out = arena.allocate((long) dim * Float.BYTES);
                int rc = (int) embed.invokeExact(textSeg, out, dim);
                if (rc != dim) {
                    throw new IllegalStateException("needle_embed 写入失败，期望 " + dim
                            + " 维，实际返回 " + rc);
                }
                float[] vector = new float[dim];
                for (int i = 0; i < dim; i++) {
                    vector[i] = out.get(ValueLayout.JAVA_FLOAT, (long) i * Float.BYTES);
                }
                return vector;
            }
        } catch (IllegalStateException | UnsupportedOperationException e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("needle_embed 调用异常：" + t, t);
        } finally {
            ENGINE_LOCK.unlock();
        }
    }

    /**
     * 清空对话历史，保留工具与权重。
     */
    static void reset() {
        ENGINE_LOCK.lock();
        try {
            if (sessionReady) {
                try {
                    needleReset.invokeExact();
                } catch (Throwable t) {
                    throw new IllegalStateException("needle_reset 调用异常：" + t, t);
                }
            }
        } finally {
            ENGINE_LOCK.unlock();
        }
    }

    /**
     * 定位并绑定权重归档，每个 JVM 进程至多成功一次。
     */
    private static void ensureWeights() {
        if (weightsLoaded) {
            return;
        }
        Path archive = resolveWeights();
        if (archive == null) {
            throw new IllegalStateException("未找到 needle 权重归档（.cact）。该文件不随本仓库分发，"
                    + "请从 huggingface.co/Cactus-Compute/needle3 获取后，通过 -D" + PROP_WEIGHTS
                    + "=<文件> 或环境变量 " + ENV_WEIGHTS + " 指定。");
        }
        byte[] bytes;
        try {
            bytes = Files.readAllBytes(archive);
        } catch (IOException e) {
            throw new IllegalStateException("读取权重归档失败：" + archive.toAbsolutePath(), e);
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment data = arena.allocateFrom(ValueLayout.JAVA_BYTE, bytes);
            int rc = (int) needleLoad.invokeExact(data, (long) bytes.length);
            if (rc < 0) {
                throw new IllegalStateException("needle_load 失败，错误码 " + rc
                        + "；归档可能不是受支持的 .cact 格式");
            }
        } catch (IllegalStateException e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("needle_load 调用异常：" + t, t);
        }
        weightsLoaded = true;
    }

    /**
     * 解析权重归档路径。
     *
     * <p>优先使用显式配置；未配置时在引擎库同目录查找 {@code *.cact}，
     * 与官方缓存布局（引擎与权重同目录）一致。</p>
     *
     * @return 权重归档路径；找不到时返回 null
     */
    private static Path resolveWeights() {
        String configured = System.getProperty(PROP_WEIGHTS);
        if (configured == null || configured.isBlank()) {
            configured = System.getenv(ENV_WEIGHTS);
        }
        if (configured != null && !configured.isBlank()) {
            Path explicit = Path.of(configured);
            if (Files.isRegularFile(explicit)) {
                return explicit;
            }
            throw new IllegalStateException("配置的权重归档不存在：" + explicit.toAbsolutePath());
        }
        Path dir = libraryDir;
        if (dir == null || !Files.isDirectory(dir)) {
            return null;
        }
        try (Stream<Path> entries = Files.list(dir)) {
            List<Path> found = entries
                    .filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString()
                            .toLowerCase(Locale.ROOT).endsWith(".cact"))
                    .toList();
            return found.isEmpty() ? null : found.get(0);
        } catch (IOException e) {
            return null;
        }
    }

    /**
     * 读取输出缓冲区大小配置。
     *
     * @return 缓冲区字节数
     */
    private static int bufferSize() {
        String configured = System.getProperty(PROP_BUFFER_SIZE);
        if (configured == null || configured.isBlank()) {
            configured = System.getenv(ENV_BUFFER_SIZE);
        }
        if (configured == null || configured.isBlank()) {
            return DEFAULT_BUFFER_SIZE;
        }
        try {
            int parsed = Integer.parseInt(configured.trim());
            if (parsed > 0) {
                return parsed;
            }
        } catch (NumberFormatException ignored) {
            // 落到默认值
        }
        return DEFAULT_BUFFER_SIZE;
    }
}
