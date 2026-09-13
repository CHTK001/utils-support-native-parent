package com.chua.nativewechat.support;

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

/**
 * 微信 4.x WCDB 自研原生库 FFM 桥接器。
 *
 * <p>基于 Java 25 FFM（Foreign Function &amp; Memory）API 绑定本模块随 jar 分发的
 * 自研动态库（{@code wechat_wcdb.dll} / {@code libwechat_wcdb.so}，Rust + SQLCipher
 * 静态链接产物，零第三方运行时依赖），替代微信闭源 {@code wcdb_api.dll} 路径。
 * 绑定的 C ABI 与 Rust 侧 {@code src/main/rust/src/lib.rs} 完全对齐：</p>
 * <ul>
 *   <li>{@code int wechat_wcdb_open_account(const char* path, const char* key, int64* h)}</li>
 *   <li>{@code int wechat_wcdb_close_account(int64 h)}</li>
 *   <li>{@code int wechat_wcdb_get_sessions(int64 h, void** out)}</li>
 *   <li>{@code int wechat_wcdb_get_messages(int64 h, const char* username, int limit, int offset, void** out)}</li>
 *   <li>{@code int wechat_wcdb_get_message_count(int64 h, const char* username, int* out)}</li>
 *   <li>{@code int wechat_wcdb_get_display_names(int64 h, const char* json, void** out)}</li>
 *   <li>{@code void wechat_wcdb_free_string(void* p)}</li>
 *   <li>{@code const char* wechat_wcdb_last_error()}</li>
 * </ul>
 *
 * <h3>平台支持</h3>
 * <p>库文件随 jar 打包在 {@code /native/{platform}/} 下，当前预编译
 * {@code windows-x86_64} 与 {@code linux-x86_64} 两个平台；调用 {@link #load()}
 * 时自动从 classpath 抽取并按平台加载，无需外部配置原生库目录。</p>
 *
 * <h3>与闭源 wcdb_api.dll 的区别</h3>
 * <ul>
 *   <li>无 {@code WCDB.dll} / {@code SDL2.dll} 依赖，无 {@code electron.exe}
 *       宿主进程名校验，普通 JVM 直接可用；</li>
 *   <li>跨平台（Windows / Linux），错误信息可通过 {@link #lastError()} 获取；</li>
 *   <li>打开失败或查询失败时返回非 0 码，调用方可读取 {@link #lastError()} 定位原因。</li>
 * </ul>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class WechatWcdbBridge implements AutoCloseable {

    /**
     * 原生函数返回成功
     */
    public static final int RC_OK = 0;

    /**
     * 参数错误返回码
     */
    public static final int RC_ARG = 1;

    /**
     * 打开/解密失败返回码
     */
    public static final int RC_OPEN = 2;

    /**
     * 查询失败返回码
     */
    public static final int RC_QUERY = 3;

    /**
     * 原生库逻辑名（无平台前缀/后缀）
     */
    private static final String LIBRARY_NAME = "wechat_wcdb";

    /**
     * 原生下行调用链接器
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 桥接器持有的共享内存会话（动态库与符号生命周期）
     */
    private final Arena arena;

    /**
     * 原生符号查找表
     */
    private final SymbolLookup lookup;

    /**
     * wechat_wcdb_open_account 函数句柄
     */
    private final MethodHandle openAccountHandle;

    /**
     * wechat_wcdb_close_account 函数句柄
     */
    private final MethodHandle closeAccountHandle;

    /**
     * wechat_wcdb_get_sessions 函数句柄
     */
    private final MethodHandle getSessionsHandle;

    /**
     * wechat_wcdb_get_messages 函数句柄
     */
    private final MethodHandle getMessagesHandle;

    /**
     * wechat_wcdb_get_message_count 函数句柄
     */
    private final MethodHandle getMessageCountHandle;

    /**
     * wechat_wcdb_get_display_names 函数句柄
     */
    private final MethodHandle getDisplayNamesHandle;

    /**
     * wechat_wcdb_free_string 函数句柄
     */
    private final MethodHandle freeStringHandle;

    /**
     * wechat_wcdb_last_error 函数句柄
     */
    private final MethodHandle lastErrorHandle;

    /**
     * 判断当前平台是否有预编译动态库。
     *
     * @return 支持返回 true
     */
    public static boolean isSupported() {
        String os = NativeUtils.getOsPrefixName();
        String arch = NativeUtils.getArchName();
        return ("windows".equals(os) || "linux".equals(os)) && "x86_64".equals(arch);
    }

    /**
     * 从 classpath 抽取并加载自研动态库，绑定全部原生函数。
     *
     * <p>库文件位于 {@code /native/{platform}/}，通过 {@link NativeLoader} 按
     * 平台抽取到临时目录后以 {@link SymbolLookup#libraryLookup(Path, Arena)}
     * 加载，随后绑定 8 个 {@code wechat_wcdb_*} 导出符号。</p>
     *
     * @return 桥接器实例
     * @throws IllegalStateException 平台不支持、库缺失或符号绑定失败时抛出
     */
    public static WechatWcdbBridge load() {
        if (!isSupported()) {
            throw new IllegalStateException("当前平台无预编译 wechat_wcdb 动态库，支持平台: windows-x86_64, linux-x86_64");
        }
        String libFileName = NativeUtils.getLibraryFileName(LIBRARY_NAME, true);
        Path targetDir = NativeUtils.tempRoot().resolve("wechat_wcdb");

        // 从 classpath 抽取本平台动态库（仅抽取，由 FFM libraryLookup 自行加载）
        NativeLoader.of("wechat_wcdb")
                .toTarget(targetDir)
                .glob(libFileName)
                .extractOnly(true)
                .load();

        Path libFile = targetDir.resolve(libFileName);
        if (!Files.isRegularFile(libFile)) {
            throw new IllegalStateException("抽取 wechat_wcdb 动态库失败: " + libFile.toAbsolutePath());
        }

        Arena sharedArena = Arena.ofShared();
        try {
            SymbolLookup lookup = SymbolLookup.libraryLookup(libFile, sharedArena);
            WechatWcdbBridge bridge = new WechatWcdbBridge(sharedArena, lookup);
            log.info("微信 WCDB 自研原生库加载成功: {}", libFile.toAbsolutePath());
            return bridge;
        } catch (RuntimeException | Error e) {
            sharedArena.close();
            throw new IllegalStateException("加载 wechat_wcdb 动态库失败: " + e.getMessage(), e);
        }
    }

    /**
     * 私有构造器，绑定全部原生函数句柄。
     *
     * @param arena  共享内存会话
     * @param lookup 原生符号查找表
     */
    private WechatWcdbBridge(Arena arena, SymbolLookup lookup) {
        this.arena = arena;
        this.lookup = lookup;
        this.openAccountHandle = bind(lookup, "wechat_wcdb_open_account",
                ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.closeAccountHandle = bind(lookup, "wechat_wcdb_close_account",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG);
        this.getSessionsHandle = bind(lookup, "wechat_wcdb_get_sessions",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.getMessagesHandle = bind(lookup, "wechat_wcdb_get_messages",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.ADDRESS);
        this.getMessageCountHandle = bind(lookup, "wechat_wcdb_get_message_count",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.getDisplayNamesHandle = bind(lookup, "wechat_wcdb_get_display_names",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.freeStringHandle = bindVoid(lookup, "wechat_wcdb_free_string", ValueLayout.ADDRESS);
        this.lastErrorHandle = bind(lookup, "wechat_wcdb_last_error", ValueLayout.ADDRESS);
    }

    /**
     * 打开微信账号会话库。
     *
     * <p>仅以只读方式打开，传入 {@code session.db} 绝对路径与 64 位十六进制
     * 原始密钥；成功后会自行发现并 ATTACH 相邻的 message / contact 分片库。</p>
     *
     * @param dbPath session.db 绝对路径
     * @param key    64 位十六进制数据库密钥
     * @return 账号库句柄（非 0，需要调用方保存并配合其余查询方法使用）
     * @throws IllegalStateException 打开/解密失败时抛出
     */
    public long openAccount(String dbPath, String key) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment dbSegment = confined.allocateFrom(dbPath, StandardCharsets.UTF_8);
            MemorySegment keySegment = confined.allocateFrom(key, StandardCharsets.UTF_8);
            MemorySegment handleOut = confined.allocate(ValueLayout.JAVA_LONG, 0L);
            int rc = (int) openAccountHandle.invokeExact(dbSegment, keySegment, handleOut);
            if (rc != RC_OK) {
                throw new IllegalStateException("wechat_wcdb_open_account 失败, code=" + rc
                        + "（请检查密钥是否正确、是否为微信 4.x 数据目录）: " + lastError());
            }
            return handleOut.get(ValueLayout.JAVA_LONG, 0L);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("wechat_wcdb_open_account 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 关闭账号库句柄并释放原生资源。
     *
     * @param handle 账号库句柄
     */
    public void closeAccount(long handle) {
        try {
            int rc = (int) closeAccountHandle.invokeExact(handle);
            if (rc != RC_OK) {
                log.warn("wechat_wcdb_close_account 返回非零码: {}", rc);
            }
        } catch (Throwable t) {
            log.warn("wechat_wcdb_close_account 调用异常: {}", t.getMessage());
        }
    }

    /**
     * 获取全部会话列表 JSON。
     *
     * <p>返回数组结构，每个会话对象包含会话表原样列，并统一补齐
     * {@code username}（会话标识）与 {@code display_name}（显示名）字段。</p>
     *
     * @param handle 账号库句柄
     * @return 会话列表 JSON 字符串（数组结构）
     * @throws IllegalStateException 查询失败时抛出
     */
    public String getSessions(long handle) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment outPointer = confined.allocate(ValueLayout.ADDRESS);
            int rc = (int) getSessionsHandle.invokeExact(handle, outPointer);
            return readOutString(rc, outPointer, "wechat_wcdb_get_sessions");
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("wechat_wcdb_get_sessions 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 分页获取指定会话的消息 JSON。
     *
     * <p>跨 message 分片库 UNION ALL 查询，按时间升序返回；每条消息统一补齐
     * {@code sender_username} / {@code local_type} / {@code create_time} /
     * {@code message_content} / {@code compress_content} 字段。</p>
     *
     * @param handle   账号库句柄
     * @param username 会话标识（wxid / 群 id）
     * @param limit    单页条数（默认惯例 500，非正数按 500 处理）
     * @param offset   偏移量（负数按 0 处理）
     * @return 消息列表 JSON 字符串
     * @throws IllegalStateException 查询失败时抛出
     */
    public String getMessages(long handle, String username, int limit, int offset) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment usernameSegment = confined.allocateFrom(username, StandardCharsets.UTF_8);
            MemorySegment outPointer = confined.allocate(ValueLayout.ADDRESS);
            int rc = (int) getMessagesHandle.invokeExact(handle, usernameSegment, limit, offset, outPointer);
            return readOutString(rc, outPointer, "wechat_wcdb_get_messages");
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("wechat_wcdb_get_messages 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 获取指定会话的消息总数（跨全部分片求和）。
     *
     * @param handle   账号库句柄
     * @param username 会话标识
     * @return 消息总数
     * @throws IllegalStateException 查询失败时抛出
     */
    public int getMessageCount(long handle, String username) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment usernameSegment = confined.allocateFrom(username, StandardCharsets.UTF_8);
            MemorySegment countOut = confined.allocate(ValueLayout.JAVA_INT, 0);
            int rc = (int) getMessageCountHandle.invokeExact(handle, usernameSegment, countOut);
            if (rc != RC_OK) {
                throw new IllegalStateException("wechat_wcdb_get_message_count 失败, code=" + rc
                        + ": " + lastError());
            }
            return countOut.get(ValueLayout.JAVA_INT, 0L);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("wechat_wcdb_get_message_count 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 批量解析发送者的显示名称。
     *
     * @param handle    账号库句柄
     * @param wxidsJson 发送者标识 JSON 数组，如 {@code ["wxid_xxx"]}（也容忍对象数组）
     * @return 标识到显示名的映射 JSON 字符串（对象结构）
     * @throws IllegalStateException 查询失败时抛出
     */
    public String getDisplayNames(long handle, String wxidsJson) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment jsonSegment = confined.allocateFrom(wxidsJson, StandardCharsets.UTF_8);
            MemorySegment outPointer = confined.allocate(ValueLayout.ADDRESS);
            int rc = (int) getDisplayNamesHandle.invokeExact(handle, jsonSegment, outPointer);
            return readOutString(rc, outPointer, "wechat_wcdb_get_display_names");
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("wechat_wcdb_get_display_names 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 获取最近一次原生错误信息（线程局部，无需释放）。
     *
     * @return 错误信息；无错误时返回 null
     */
    public String lastError() {
        try {
            MemorySegment pointer = (MemorySegment) lastErrorHandle.invokeExact();
            if (pointer == null || pointer.address() == 0L) {
                return null;
            }
            return pointer.reinterpret(Long.MAX_VALUE).getString(0L, StandardCharsets.UTF_8);
        } catch (Throwable t) {
            log.warn("wechat_wcdb_last_error 调用异常: {}", t.getMessage());
            return null;
        }
    }

    @Override
    public void close() {
        arena.close();
    }

    /**
     * 读取原生出参指针指向的字符串并释放原生内存。
     *
     * @param rc           原生函数返回码
     * @param outSlot      出参指针槽
     * @param functionName 函数名（异常信息用）
     * @return UTF-8 字符串；原生返回空指针时返回 null
     */
    private String readOutString(int rc, MemorySegment outSlot, String functionName) {
        if (rc != RC_OK) {
            throw new IllegalStateException(functionName + " 失败, code=" + rc + ": " + lastError());
        }
        MemorySegment pointer = outSlot.get(ValueLayout.ADDRESS, 0L);
        if (pointer.address() == 0L) {
            return null;
        }
        // 原生字符串以 '\0' 结尾，放开段边界后按 UTF-8 读取
        String result = pointer.reinterpret(Long.MAX_VALUE).getString(0L, StandardCharsets.UTF_8);
        freeString(pointer);
        return result;
    }

    /**
     * 释放原生层分配的字符串。
     *
     * @param pointer 原生字符串指针
     */
    private void freeString(MemorySegment pointer) {
        try {
            freeStringHandle.invokeExact(pointer);
        } catch (Throwable t) {
            log.warn("wechat_wcdb_free_string 调用异常: {}", t.getMessage());
        }
    }

    /**
     * 绑定有返回值的原生函数。
     *
     * @param lookup       符号查找表
     * @param name         函数符号名
     * @param returnLayout 返回值布局
     * @param argLayouts   参数布局
     * @return 下行调用句柄
     */
    private static MethodHandle bind(SymbolLookup lookup, String name,
                                     ValueLayout returnLayout, ValueLayout... argLayouts) {
        MemorySegment symbol = lookup.find(name)
                .orElseThrow(() -> new IllegalStateException("wechat_wcdb 动态库缺少导出符号: " + name));
        return LINKER.downcallHandle(symbol, FunctionDescriptor.of(returnLayout, argLayouts));
    }

    /**
     * 绑定无返回值的原生函数。
     *
     * @param lookup     符号查找表
     * @param name       函数符号名
     * @param argLayouts 参数布局
     * @return 下行调用句柄
     */
    private static MethodHandle bindVoid(SymbolLookup lookup, String name, ValueLayout... argLayouts) {
        MemorySegment symbol = lookup.find(name)
                .orElseThrow(() -> new IllegalStateException("wechat_wcdb 动态库缺少导出符号: " + name));
        return LINKER.downcallHandle(symbol, FunctionDescriptor.ofVoid(argLayouts));
    }
}
