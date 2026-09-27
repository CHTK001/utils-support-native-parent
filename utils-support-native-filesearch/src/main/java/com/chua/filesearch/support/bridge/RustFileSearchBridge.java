package com.chua.filesearch.support.bridge;

import com.chua.common.support.lang.json.Json5;
import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import lombok.extern.slf4j.Slf4j;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.function.Consumer;
import java.util.regex.Pattern;
import java.util.stream.Stream;

/**
 * Rust 文件搜索原生库桥接（Panama FFM）。
 *
 * <p>加载 {@code file_search} 动态库并调用其扁平 C ABI，提供按名称搜索文件与
 * 遍历目录树的能力。</p>
 *
 * <h3>为什么用 FFM 而不是 JNI</h3>
 * <p>该库此前由一组 {@code Java_...} 前缀的 JNI 函数对接，但那组函数并不符合 JNI
 * 约定：没有 {@code JNIEnv*} / {@code jclass} 前导参数（参数整体错位，root 实际收到
 * 的是 env 指针），{@code getVersion} 以 {@code *const c_char} 冒充 {@code jstring}
 * 返回，{@code searchByName} 的 Java 回调参数被忽略，{@code searchBySize} /
 * {@code searchByPath} 是直接返回 -1 的桩。实测后果是：调用 {@code getVersion}
 * 会让 JVM 以 EXCEPTION_ACCESS_VIOLATION 硬崩，{@code searchByName} 静默返回 0。</p>
 *
 * <p>同一份动态库另有一组正确的扁平 C ABI（{@code fast_*} 与
 * {@code Java_..._RustFileSearchBridge__raw*}），无需 JNI 环境、直接接收 C 字符串
 * 并返回 JSON。改为经 FFM 调用这组符号后，上述缺陷不再可达，且无需重建原生库。</p>
 *
 * <h3>原生侧能力与限制</h3>
 * <ul>
 *   <li>遍历深度在原生侧硬编码为 3，{@link #getTree} 的 depth 参数不生效；</li>
 *   <li>原生遍历会跳过目录项，故 {@link FileResultData#isDirectory()} 恒为
 *       {@code false}，{@link #getTree} 返回的实际是文件列表而非树；</li>
 *   <li>JSON 仅含 path / size / modified / ext 四项，其余组件按默认值填充；</li>
 *   <li>{@link #searchBySize} 与 {@link #searchByPath} 原生无对应能力（原为返回 -1
 *       的桩），改由本类在全量结果上做 Java 侧过滤实现。</li>
 * </ul>
 *
 * @author CH
 * @since 4.0.0.43
 */
@Slf4j
public final class RustFileSearchBridge {

    /**
     * 原生库逻辑名，用于 classpath 抽取与文件名匹配
     */
    private static final String LIBRARY_NAME = "file_search";

    /**
     * 库生命周期作用域：进程级共享，进程内不卸载
     */
    private static final Arena LIB_ARENA = Arena.global();

    /**
     * 原生链接器
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 加载锁
     */
    private static final Object LOCK = new Object();

    /**
     * 按名称搜索（返回 JSON）的句柄
     */
    private static volatile MethodHandle rawSearchByName;

    /**
     * 遍历目录树（返回 JSON）的句柄
     */
    private static volatile MethodHandle rawGetTree;

    /**
     * 取版本号的句柄
     */
    private static volatile MethodHandle fastGetVersion;

    /**
     * 取消搜索的句柄
     */
    private static volatile MethodHandle fastCancel;

    /**
     * 是否已加载
     */
    private static volatile boolean loaded = false;

    /**
     * 加载失败原因；加载成功时为 null
     */
    private static volatile Throwable loadError;

    /**
     * 工具类，禁止实例化。
     */
    private RustFileSearchBridge() {
    }

    /**
     * 文件条目。
     *
     * <p>字段与原 JNI 版本保持一致以免影响调用方；但原生 JSON 只提供 path / size /
     * modified / ext 四项，故 isDirectory 恒为 false，attributes / usnRecordId /
     * parentFileId / allocatedSize 恒为 0。</p>
     *
     * @param path            文件路径（原生已把反斜杠规范化为斜杠）
     * @param size            字节数
     * @param lastModified    最后修改时间（Unix 毫秒）
     * @param isDirectory     是否目录（原生不返回目录，恒为 false）
     * @param extension       扩展名（小写，无扩展名时为空串）
     * @param attributes      文件属性位（原生未提供，恒为 0）
     * @param usnRecordId     USN 记录号（原生未提供，恒为 0）
     * @param parentFileId    父目录文件号（原生未提供，恒为 0）
     * @param allocatedSize   分配大小（原生未提供，恒为 0）
     */
    public record FileResultData(String path, long size, long lastModified, boolean isDirectory,
                                 String extension, int attributes, long usnRecordId,
                                 long parentFileId, long allocatedSize) {}

    /**
     * 加载原生库并绑定符号。线程安全，重复调用无副作用。
     */
    public static synchronized void loadLibrary() {
        if (loaded) {
            return;
        }
        synchronized (LOCK) {
            if (loaded) {
                return;
            }
            try {
                Path targetDir = NativeUtils.tempRoot().resolve("file-search");
                NativeLoader.of("file-search")
                        .toTarget(targetDir)
                        .glob("*" + LIBRARY_NAME + "*")
                        .extractOnly(true)
                        .load();
                Path libPath = findExtractedLibrary(targetDir);
                SymbolLookup lookup = SymbolLookup.libraryLookup(libPath, LIB_ARENA);
                rawSearchByName = downcall(lookup,
                        "Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName",
                        FunctionDescriptor.of(ValueLayout.ADDRESS,
                                ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
                rawGetTree = downcall(lookup,
                        "Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawGetTree",
                        FunctionDescriptor.of(ValueLayout.ADDRESS,
                                ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT));
                fastGetVersion = downcall(lookup, "fast_get_version",
                        FunctionDescriptor.of(ValueLayout.ADDRESS));
                fastCancel = downcall(lookup, "fast_search_cancel",
                        FunctionDescriptor.ofVoid());
                loaded = true;
                loadError = null;
                log.info("[filesearch] 原生库加载成功（FFM），版本 {}", getVersion());
            } catch (Throwable e) {
                loadError = e;
                loaded = false;
                log.warn("[filesearch] 原生库不可用，将由调用方回退 JDK 遍历: {}", e.getMessage());
            }
        }
    }

    /**
     * 在抽取目录内定位动态库。
     *
     * @param targetDir 抽取目录
     * @return 动态库路径
     */
    private static Path findExtractedLibrary(Path targetDir) {
        try (Stream<Path> entries = Files.list(targetDir)) {
            return entries
                    .filter(Files::isRegularFile)
                    .filter(p -> p.getFileName().toString().toLowerCase().contains(LIBRARY_NAME))
                    .findFirst()
                    .orElseThrow(() -> new IllegalStateException(
                            "抽取目录内没有 " + LIBRARY_NAME + " 动态库：" + targetDir));
        } catch (java.io.IOException e) {
            throw new IllegalStateException("读取抽取目录失败：" + targetDir, e);
        }
    }

    /**
     * 绑定一个导出符号。
     *
     * @param lookup     符号查找器
     * @param symbol     符号名
     * @param descriptor 函数签名
     * @return 下调方法句柄
     */
    private static MethodHandle downcall(SymbolLookup lookup, String symbol,
                                         FunctionDescriptor descriptor) {
        return LINKER.downcallHandle(
                lookup.find(symbol).orElseThrow(() -> new UnsatisfiedLinkError(symbol + " not found")),
                descriptor);
    }

    /**
     * 查询是否已加载原生库。
     *
     * @return 已加载返回 true
     */
    public static boolean isLoaded() {
        return loaded;
    }

    /**
     * 获取原生库加载失败原因。
     *
     * @return 失败异常；加载成功或未尝试时返回 null
     */
    public static Throwable getLoadError() {
        return loadError;
    }

    /**
     * 按名称搜索文件。
     *
     * @param root     搜索根目录
     * @param pattern  文件名 glob 模式，可为 空 表示不过滤
     * @param max      最大返回数，&lt;= 0 表示不限
     * @param cb       每个匹配文件的回调，可为 空
     * @return 匹配数量；原生调用失败返回 -1
     */
    public static int searchByName(String root, String pattern, int max,
                                   Consumer<FileResultData> cb) {
        return callRaw(rawSearchByName, root, pattern, max, cb);
    }

    /**
     * 遍历目录树。
     *
     * <p>原生侧遍历深度硬编码为 3，且会跳过目录项，故本方法实际返回文件列表。
     * depth 参数保留仅为签名兼容，不生效。</p>
     *
     * @param root  根目录
     * @param depth 最大深度（原生不支持，忽略）
     * @param max   最大返回数，&lt;= 0 表示不限
     * @param cb    每个条目的回调，可为 空
     * @return 条目数量；原生调用失败返回 -1
     */
    public static int getTree(String root, int depth, int max, Consumer<FileResultData> cb) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment rootSeg = arena.allocateFrom(root, StandardCharsets.UTF_8);
            MemorySegment ptr = (MemorySegment) rawGetTree.invokeExact(rootSeg, depth, max);
            return consume(ptr, cb);
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("getTree 调用失败", e);
        }
    }

    /**
     * 按文件大小区间搜索。
     *
     * <p>原生无此能力（原实现是返回 -1 的桩），此处对全量结果做 Java 侧过滤。</p>
     *
     * @param root    搜索根目录
     * @param minSize 最小字节数（含）
     * @param maxSize 最大字节数（含）
     * @param max     最大返回数，&lt;= 0 表示不限
     * @param cb      每个匹配文件的回调，可为 空
     * @return 匹配数量；原生调用失败返回 -1
     */
    public static int searchBySize(String root, long minSize, long maxSize, int max,
                                   Consumer<FileResultData> cb) {
        List<FileResultData> all = new java.util.ArrayList<>();
        int count = callRaw(rawSearchByName, root, null, 0, all::add);
        if (count < 0) {
            return -1;
        }
        int hit = 0;
        for (FileResultData data : all) {
            if (data.size() < minSize || data.size() > maxSize) {
                continue;
            }
            if (max > 0 && hit >= max) {
                break;
            }
            hit++;
            if (cb != null) {
                cb.accept(data);
            }
        }
        return hit;
    }

    /**
     * 按路径模式搜索。
     *
     * <p>原生无此能力（原实现是返回 -1 的桩），此处对全量结果按完整路径做 glob
     * 过滤；匹配不区分大小写，且 * 可跨越路径分隔符。</p>
     *
     * @param root    搜索根目录
     * @param pattern 路径 glob 模式，可为 空 表示不过滤
     * @param max     最大返回数，&lt;= 0 表示不限
     * @param cb      每个匹配文件的回调，可为 空
     * @return 匹配数量；原生调用失败返回 -1
     */
    public static int searchByPath(String root, String pattern, int max,
                                   Consumer<FileResultData> cb) {
        List<FileResultData> all = new java.util.ArrayList<>();
        int count = callRaw(rawSearchByName, root, null, 0, all::add);
        if (count < 0) {
            return -1;
        }
        Pattern matcher = pattern == null || pattern.isEmpty() ? null : globToPattern(pattern);
        int hit = 0;
        for (FileResultData data : all) {
            if (matcher != null && !matcher.matcher(data.path()).matches()) {
                continue;
            }
            if (max > 0 && hit >= max) {
                break;
            }
            hit++;
            if (cb != null) {
                cb.accept(data);
            }
        }
        return hit;
    }

    /**
     * 读取原生库版本号。
     *
     * @return 版本串；未加载或读取失败时返回空串
     */
    public static String getVersion() {
        if (!loaded) {
            return "";
        }
        try {
            MemorySegment ptr = (MemorySegment) fastGetVersion.invokeExact();
            if (ptr == null || ptr.equals(MemorySegment.NULL)) {
                return "";
            }
            // 原生返回的是不带长度的 C 字符串，需先给出有界窗口再读
            return ptr.reinterpret(64).getString(0, StandardCharsets.UTF_8);
        } catch (Throwable e) {
            return "";
        }
    }

    /**
     * 取消正在进行的搜索。未加载时为空操作。
     */
    public static void cancel() {
        if (!loaded) {
            return;
        }
        try {
            fastCancel.invokeExact();
        } catch (Throwable e) {
            log.warn("[filesearch] cancel 调用失败: {}", e.getMessage());
        }
    }

    /**
     * 安全封装：加载库后按名称搜索文件。
     *
     * @param rootPath     搜索根目录
     * @param namePattern  文件名模式（支持 glob）
     * @param maxResults   最大返回结果数
     * @param callback     每个匹配文件的回调
     * @return 匹配数量
     */
    public static int searchByNameSafe(String rootPath, String namePattern, int maxResults,
                                       Consumer<FileResultData> callback) {
        loadLibrary();
        return searchByName(rootPath, namePattern, maxResults, callback);
    }

    /**
     * 安全封装：加载库后遍历目录树。
     *
     * @param rootPath   根目录
     * @param maxDepth   最大深度（原生不支持，忽略）
     * @param maxResults 最大返回结果数
     * @param callback   每个节点的回调
     * @return 遍历节点数量
     */
    public static int getTreeSafe(String rootPath, int maxDepth, int maxResults,
                                  Consumer<FileResultData> callback) {
        loadLibrary();
        return getTree(rootPath, maxDepth, maxResults, callback);
    }

    /**
     * 校验已加载。
     */
    private static void requireLoaded() {
        if (!loaded) {
            throw new IllegalStateException("file_search 原生库未加载："
                    + (loadError == null ? "未尝试加载" : loadError.getMessage()));
        }
    }

    /**
     * 调用返回 JSON 的原生搜索函数。
     *
     * @param handle  原生函数句柄
     * @param root    根目录
     * @param pattern 名称模式，可为 空
     * @param max     最大返回数
     * @param cb      回调，可为 空
     * @return 匹配数量；失败返回 -1
     */
    private static int callRaw(MethodHandle handle, String root, String pattern, int max,
                               Consumer<FileResultData> cb) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment rootSeg = arena.allocateFrom(root, StandardCharsets.UTF_8);
            MemorySegment patSeg = pattern == null || pattern.isEmpty()
                    ? MemorySegment.NULL
                    : arena.allocateFrom(pattern, StandardCharsets.UTF_8);
            MemorySegment ptr = (MemorySegment) handle.invokeExact(rootSeg, patSeg, max);
            return consume(ptr, cb);
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("file_search 原生调用失败", e);
        }
    }

    /**
     * 解析原生返回的 JSON envelope 并回调。
     *
     * @param ptr 原生返回的 C 字符串指针
     * @param cb  回调，可为 空
     * @return count 字段；失败返回 -1
     */
    @SuppressWarnings("unchecked")
    private static int consume(MemorySegment ptr, Consumer<FileResultData> cb) {
        if (ptr == null || ptr.equals(MemorySegment.NULL)) {
            return -1;
        }
        String json = ptr.reinterpret(Long.MAX_VALUE).getString(0, StandardCharsets.UTF_8);
        Map<String, Object> envelope = Json5.fromJson(json);
        Object results = envelope.get("results");
        if (results instanceof List<?> list && cb != null) {
            for (Object item : list) {
                if (item instanceof Map<?, ?> map) {
                    cb.accept(toResult((Map<String, Object>) map));
                }
            }
        }
        Object count = envelope.get("count");
        return count instanceof Number ? ((Number) count).intValue() : -1;
    }

    /**
     * 把原生 JSON 条目转成 FileResultData。
     *
     * @param map 原生条目
     * @return 文件条目
     */
    private static FileResultData toResult(Map<String, Object> map) {
        String path = asString(map.get("path"));
        long size = asLong(map.get("size"));
        long modified = asLong(map.get("modified"));
        String ext = asString(map.get("ext"));
        return new FileResultData(path, size, modified, false, ext, 0, 0L, 0L, 0L);
    }

    /**
     * 取字符串字段。
     *
     * @param value 原值
     * @return 字符串；为空时返回空串
     */
    private static String asString(Object value) {
        return value == null ? "" : String.valueOf(value);
    }

    /**
     * 取长整数字段。
     *
     * @param value 原值
     * @return 长整数；非数字时返回 0
     */
    private static long asLong(Object value) {
        return value instanceof Number ? ((Number) value).longValue() : 0L;
    }

    /**
     * 把 glob 模式编译为正则。
     *
     * <p>支持 * 与 ?；* 可跨越路径分隔符，匹配不区分大小写。</p>
     *
     * @param glob glob 模式
     * @return 正则
     */
    private static Pattern globToPattern(String glob) {
        StringBuilder sb = new StringBuilder("^");
        for (char c : glob.toCharArray()) {
            switch (c) {
                case '*':
                    sb.append(".*");
                    break;
                case '?':
                    sb.append('.');
                    break;
                case '.': case '(': case ')': case '+': case '|': case '^': case '$':
                case '@': case '%': case '{': case '}': case '[': case ']': case '\\':
                    sb.append('\\').append(c);
                    break;
                default:
                    sb.append(c);
            }
        }
        return Pattern.compile(sb.append('$').toString(), Pattern.CASE_INSENSITIVE);
    }
}
