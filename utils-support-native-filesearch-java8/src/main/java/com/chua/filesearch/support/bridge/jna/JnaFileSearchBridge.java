package com.chua.filesearch.support.bridge.jna;

import com.sun.jna.Memory;
import com.sun.jna.Native;
import com.sun.jna.Pointer;
import lombok.extern.slf4j.Slf4j;

import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.Consumer;
import java.util.regex.Pattern;

/**
 * Rust 文件搜索原生库桥接（Java 8 + JNA）。
 *
 * <p>与 Java 25 侧 {@code RustFileSearchBridge}（Panama FFM）能力对齐：加载
 * {@code file_search} 动态库并调用其扁平 C ABI，提供按名称搜索文件与遍历目录树。
 * JNA 是纯 Java 库，{@code net.java.dev.jna:jna} 支持 Java 8，因此本模块的产物
 * 可在 JDK 8 上直接运行，而主模块因使用 {@code record} 与 FFM 只能运行于 JDK 25。</p>
 *
 * <h3>与 FFM 版的差异</h3>
 * <ul>
 *   <li>结果类型是 {@link FileSearchResult}（普通只读类），不是 {@code record}
 *       {@code FileResultData}；</li>
 *   <li>JSON 解析走内置 {@link SearchJsonParser}，不依赖 {@code Json5}；</li>
 *   <li>参数按 UTF-8 手工编组，避免 JNA 默认平台编码在 Windows 上把中文路径按 GBK
 *       传给原生（原生按 UTF-8 解码）。</li>
 * </ul>
 *
 * <h3>原生侧能力与限制（与 FFM 版一致）</h3>
 * <ul>
 *   <li>遍历深度在原生侧硬编码为 3，{@link #getTree} 的 depth 参数不生效；</li>
 *   <li>原生遍历跳过目录项，故 {@link FileSearchResult#isDirectory()} 恒为
 *       {@code false}，{@link #getTree} 实际返回文件列表；</li>
 *   <li>JSON 仅含 path / size / modified / ext 四项；</li>
 *   <li>{@link #searchBySize} 与 {@link #searchByPath} 原生无对应能力，改由本类在全量
 *       结果上做 Java 侧过滤实现。</li>
 * </ul>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class JnaFileSearchBridge {

    /**
     * 加载锁
     */
    private static final Object LOCK = new Object();

    /**
     * 已加载的 JNA 绑定实例
     */
    private static volatile FileSearchAbi abi;

    /**
     * 已释放的动态库本地路径
     */
    private static volatile Path libraryPath;

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
    private JnaFileSearchBridge() {
    }

    /**
     * 加载原生库并建立 JNA 绑定。线程安全，重复调用无副作用。
     *
     * <p>JNA 的符号解析是惰性的，因此这里主动调用一次 {@link #getVersion()}，
     * 让缺符号问题在加载阶段就暴露，而不是首次搜索时才失败。</p>
     */
    public static void loadLibrary() {
        if (loaded) {
            return;
        }
        synchronized (LOCK) {
            if (loaded) {
                return;
            }
            try {
                Path path = NativeLibraryExtractor.resolve();
                FileSearchAbi instance = Native.load(path.toString(), FileSearchAbi.class);
                abi = instance;
                libraryPath = path;
                loaded = true;
                loadError = null;
                log.info("[filesearch-jna] 原生库加载成功（JNA），版本 {}", getVersion());
            } catch (Throwable e) {
                loadError = e;
                loaded = false;
                abi = null;
                libraryPath = null;
                log.warn("[filesearch-jna] 原生库不可用，请改用 JDK 遍历或 FFM 版: {}", e.getMessage());
            }
        }
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
     * 获取已释放到本地的动态库路径。
     *
     * @return 动态库路径；未加载时返回 null
     */
    public static Path getLibraryPath() {
        return libraryPath;
    }

    /**
     * 读取原生库版本号。
     *
     * @return 版本串；未加载或读取失败时返回空串
     */
    public static String getVersion() {
        FileSearchAbi instance = abi;
        if (!loaded || instance == null) {
            return "";
        }
        try {
            Pointer pointer = instance.fast_get_version();
            if (pointer == null) {
                return "";
            }
            return pointer.getString(0, "UTF-8");
        } catch (Throwable e) {
            log.warn("[filesearch-jna] 读取版本号失败: {}", e.getMessage());
            return "";
        }
    }

    /**
     * 取消正在进行的搜索。未加载时为空操作。
     */
    public static void cancel() {
        FileSearchAbi instance = abi;
        if (!loaded || instance == null) {
            return;
        }
        try {
            instance.fast_search_cancel();
        } catch (Throwable e) {
            log.warn("[filesearch-jna] cancel 调用失败: {}", e.getMessage());
        }
    }

    /**
     * 按名称搜索文件。
     *
     * @param root    搜索根目录
     * @param pattern 文件名 glob 模式，可为 空 表示不过滤
     * @param max     最大返回数，&lt;= 0 表示不限
     * @param cb      每个匹配文件的回调，可为 空
     * @return 匹配数量；原生调用失败返回 -1
     */
    public static int searchByName(String root, String pattern, int max, Consumer<FileSearchResult> cb) {
        requireLoaded();
        Memory rootMemory = toUtf8(root);
        Memory patternMemory = toUtf8Nullable(pattern);
        Pointer pointer = abi.Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName(
                rootMemory, patternMemory, max);
        return consume(pointer, cb);
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
    public static int getTree(String root, int depth, int max, Consumer<FileSearchResult> cb) {
        requireLoaded();
        Memory rootMemory = toUtf8(root);
        Pointer pointer = abi.Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawGetTree(
                rootMemory, depth, max);
        return consume(pointer, cb);
    }

    /**
     * 按文件大小区间搜索。
     *
     * <p>原生无此能力，此处对全量结果做 Java 侧过滤。</p>
     *
     * @param root    搜索根目录
     * @param minSize 最小字节数（含）
     * @param maxSize 最大字节数（含）
     * @param max     最大返回数，&lt;= 0 表示不限
     * @param cb      每个匹配文件的回调，可为 空
     * @return 匹配数量；原生调用失败返回 -1
     */
    public static int searchBySize(String root, long minSize, long maxSize, int max,
                                   Consumer<FileSearchResult> cb) {
        List<FileSearchResult> all = new ArrayList<>();
        int count = searchByName(root, null, 0, all::add);
        if (count < 0) {
            return -1;
        }
        int hit = 0;
        for (FileSearchResult data : all) {
            if (data.getSize() < minSize || data.getSize() > maxSize) {
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
     * <p>原生无此能力，此处对全量结果按完整路径做 glob 过滤；匹配不区分大小写，
     * 且 {@code *} 可跨越路径分隔符。</p>
     *
     * @param root    搜索根目录
     * @param pattern 路径 glob 模式，可为 空 表示不过滤
     * @param max     最大返回数，&lt;= 0 表示不限
     * @param cb      每个匹配文件的回调，可为 空
     * @return 匹配数量；原生调用失败返回 -1
     */
    public static int searchByPath(String root, String pattern, int max, Consumer<FileSearchResult> cb) {
        List<FileSearchResult> all = new ArrayList<>();
        int count = searchByName(root, null, 0, all::add);
        if (count < 0) {
            return -1;
        }
        Pattern matcher = pattern == null || pattern.isEmpty() ? null : globToPattern(pattern);
        int hit = 0;
        for (FileSearchResult data : all) {
            if (matcher != null && !matcher.matcher(data.getPath()).matches()) {
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
     * 安全封装：加载库后按名称搜索文件。
     *
     * @param rootPath    搜索根目录
     * @param namePattern 文件名模式（支持 glob）
     * @param maxResults  最大返回结果数
     * @param callback    每个匹配文件的回调
     * @return 匹配数量
     */
    public static int searchByNameSafe(String rootPath, String namePattern, int maxResults,
                                       Consumer<FileSearchResult> callback) {
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
                                  Consumer<FileSearchResult> callback) {
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
     * 解析原生返回的 JSON envelope 并回调。
     *
     * @param pointer 原生返回的 C 字符串指针
     * @param cb      回调，可为 空
     * @return count 字段；失败返回 -1
     */
    private static int consume(Pointer pointer, Consumer<FileSearchResult> cb) {
        if (pointer == null) {
            return -1;
        }
        String json = pointer.getString(0, "UTF-8");
        Object parsed = SearchJsonParser.parse(json);
        if (!(parsed instanceof Map)) {
            return -1;
        }
        Map<?, ?> envelope = (Map<?, ?>) parsed;
        Object results = envelope.get("results");
        if (results instanceof List && cb != null) {
            for (Object item : (List<?>) results) {
                if (item instanceof Map) {
                    cb.accept(toResult((Map<?, ?>) item));
                }
            }
        }
        Object count = envelope.get("count");
        return count instanceof Number ? ((Number) count).intValue() : -1;
    }

    /**
     * 把原生 JSON 条目转成 {@link FileSearchResult}。
     *
     * @param map 原生条目
     * @return 文件条目
     */
    private static FileSearchResult toResult(Map<?, ?> map) {
        String path = asString(map.get("path"));
        long size = asLong(map.get("size"));
        long modified = asLong(map.get("modified"));
        String extension = asString(map.get("ext"));
        return new FileSearchResult(path, size, modified, false, extension, 0, 0L, 0L, 0L);
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
     * 把字符串编组为 UTF-8 的 NUL 结尾 C 字符串。
     *
     * @param value 字符串（非空）
     * @return JNA 内存块
     */
    private static Memory toUtf8(String value) {
        byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
        Memory memory = new Memory(bytes.length + 1L);
        memory.write(0, bytes, 0, bytes.length);
        memory.setByte(bytes.length, (byte) 0);
        return memory;
    }

    /**
     * 把可空字符串编组为 UTF-8 的 NUL 结尾 C 字符串。
     *
     * @param value 字符串，可为 空
     * @return 字符串为空时返回 null（JNA 会传 C 空指针）
     */
    private static Memory toUtf8Nullable(String value) {
        if (value == null || value.isEmpty()) {
            return null;
        }
        return toUtf8(value);
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
        StringBuilder builder = new StringBuilder("^");
        for (char c : glob.toCharArray()) {
            switch (c) {
                case '*':
                    builder.append(".*");
                    break;
                case '?':
                    builder.append('.');
                    break;
                case '.':
                case '(':
                case ')':
                case '+':
                case '|':
                case '^':
                case '$':
                case '@':
                case '%':
                case '{':
                case '}':
                case '[':
                case ']':
                case '\\':
                    builder.append('\\').append(c);
                    break;
                default:
                    builder.append(c);
            }
        }
        return Pattern.compile(builder.append('$').toString(), Pattern.CASE_INSENSITIVE);
    }
}
