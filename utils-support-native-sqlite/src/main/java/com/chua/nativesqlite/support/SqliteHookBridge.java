package com.chua.nativesqlite.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import lombok.extern.slf4j.Slf4j;

/**
 * {@code sqlite3_hook} 原生库的 FFM 绑定层。
 *
 * <p>本类只负责「怎么和原生库说话」：定位并加载动态库、解析符号、把 8 个 C 导出
 * 绑定成 {@link MethodHandle}。至于「用这些原语做什么」（同步连接语义、Reactor
 * 事件流语义）属于调用方，留在 {@code utils-support-sqlite-starter}。</p>
 *
 * <p><b>为什么在本模块而不是 starter</b>：按仓库既定分层，绑定类应与其原生库同模块
 * （对照 {@code MetricsNativeLibrary}、{@code RustSmbServerBridge}、
 * {@code NativeFFmpeg}）。本类于 2026-09-29 从 utils 侧迁入，此前绑定代码散落在
 * {@code SqliteHookConnection} 与 {@code SqliteReactorHook} 两个类里各自实现了一遍
 * {@code loadLibrary()} 与 {@code bind()}，既越层又重复。</p>
 *
 * <p><b>加载方式</b>：统一走项目受控的 {@link NativeLoader}，按平台目录抽取并做
 * MD5 去重，避免业务代码直接调用 {@code System.load}。</p>
 *
 * <p><b>线程安全</b>：加载是一次性的，用双重检查锁；句柄在首次成功加载后不再变化，
 * 读侧为 volatile，可安全并发取用。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class SqliteHookBridge {

    /**
     * 原生库逻辑名，与 {@code NativeLoader} 的 taskId 对应。
     */
    private static final String TASK_ID = "sqlite3-hook";

    /**
     * 动态库文件名（不含 lib 前缀与后缀），供 NativeUtils 推导平台文件名。
     */
    private static final String LIB_STEM = "sqlite3_hook";

    /**
     * FFM 链接器。
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 已加载库的符号查找器，加载成功后赋值。
     */
    private static volatile SymbolLookup symLookup;

    /**
     * 是否已尝试过加载（无论成败），用于避免重复尝试与重复打日志。
     */
    private static volatile boolean resolved;

    /**
     * 加载是否成功。
     */
    private static volatile boolean available;

    /**
     * {@code hook_open} 句柄。
     */
    private static volatile MethodHandle hookOpen;

    /**
     * {@code hook_poll} 句柄。
     */
    private static volatile MethodHandle hookPoll;

    /**
     * {@code hook_exec} 句柄。
     */
    private static volatile MethodHandle hookExec;

    /**
     * {@code hook_close} 句柄。
     */
    private static volatile MethodHandle hookClose;

    /**
     * {@code hook_open_async} 句柄。
     */
    private static volatile MethodHandle hookOpenAsync;

    /**
     * {@code hook_exec_async} 句柄。
     */
    private static volatile MethodHandle hookExecAsync;

    /**
     * {@code hook_close_async} 句柄。
     */
    private static volatile MethodHandle hookCloseAsync;

    /**
     * 工具类，禁止实例化。
     */
    private SqliteHookBridge() {
    }

    /**
     * 加载原生库并绑定全部导出。
     *
     * <p>幂等：重复调用只做一次实际加载。失败不抛异常，由调用方通过
     * {@link #isAvailable()} 判断并按业务降级。</p>
     *
     * @return 加载成功返回 true，否则 false
     */
    public static boolean load() {
        if (resolved) {
            return available;
        }
        synchronized (SqliteHookBridge.class) {
            if (resolved) {
                return available;
            }
            try {
                NativeLoader.of(TASK_ID)
                        .glob(NativeUtils.getLibraryFileName(LIB_STEM))
                        .toTarget(NativeUtils.tempRoot().resolve(TASK_ID).toFile().getAbsolutePath())
                        .load();
                symLookup = SymbolLookup.loaderLookup();
                hookOpen = bind("hook_open",
                        FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
                hookPoll = bind("hook_poll",
                        FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                                ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
                hookExec = bind("hook_exec",
                        FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                                ValueLayout.ADDRESS));
                hookClose = bind("hook_close",
                        FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
                hookOpenAsync = bind("hook_open_async",
                        FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                                ValueLayout.ADDRESS, ValueLayout.ADDRESS));
                hookExecAsync = bind("hook_exec_async",
                        FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                                ValueLayout.ADDRESS));
                hookCloseAsync = bind("hook_close_async",
                        FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
                available = true;
                log.debug("sqlite3_hook 原生库已加载并完成 8 个导出绑定");
            } catch (Throwable e) {
                available = false;
                log.warn("SQLite CDC 原生钩子库不可用，CDC 能力降级: {}", e.toString());
            } finally {
                resolved = true;
            }
            return available;
        }
    }

    /**
     * 原生库是否可用。首次调用会触发加载。
     *
     * @return 可用返回 true，否则 false
     */
    public static boolean isAvailable() {
        return load();
    }

    /**
     * {@code hook_open} 句柄：打开同步句柄。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookOpen() {
        load();
        return hookOpen;
    }

    /**
     * {@code hook_poll} 句柄：非阻塞读取一条事件。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookPoll() {
        load();
        return hookPoll;
    }

    /**
     * {@code hook_exec} 句柄：在已打开句柄上执行 SQL。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookExec() {
        load();
        return hookExec;
    }

    /**
     * {@code hook_close} 句柄：关闭同步句柄。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookClose() {
        load();
        return hookClose;
    }

    /**
     * {@code hook_open_async} 句柄：打开异步句柄并注册回调。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookOpenAsync() {
        load();
        return hookOpenAsync;
    }

    /**
     * {@code hook_exec_async} 句柄：在已打开异步句柄上执行 SQL。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookExecAsync() {
        load();
        return hookExecAsync;
    }

    /**
     * {@code hook_close_async} 句柄：关闭异步句柄。
     *
     * @return 方法处理 对象
     */
    public static MethodHandle hookCloseAsync() {
        load();
        return hookCloseAsync;
    }

    /**
     * 按名绑定一个导出为 downcall 句柄。
     *
     * @param name 导出名，不允许为 null
     * @param desc 函数描述符，不允许为 null
     * @return 方法处理 对象
     * @throws UnsatisfiedLinkError 符号不存在时抛出
     */
    private static MethodHandle bind(String name, FunctionDescriptor desc) {
        MemorySegment sym = symLookup.find(name)
                .orElseThrow(() -> new UnsatisfiedLinkError("符号未找到: " + name));
        return LINKER.downcallHandle(sym, desc);
    }

    /**
     * 为 {@code hook_open_async} 的事件回调创建 upcall stub。
     *
     * <p>C 侧签名是 {@code void (*)(const char *json, void *user_data)}，属于与原生库的
     * ABI 约定，因此放在绑定层而不是调用方，避免调用方各自拼 FunctionDescriptor 走样。</p>
     *
     * @param target Java 侧的目标方法，签名须为 (MemorySegment, MemorySegment) -> void
     * @param arena  承载 stub 生命周期的 Arena，调用方负责保持其存活
     * @return 可传给 {@link #hookOpenAsync()} 的函数指针
     */
    public static MemorySegment eventCallbackStub(MethodHandle target, Arena arena) {
        FunctionDescriptor fd = FunctionDescriptor.ofVoid(
                ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        return LINKER.upcallStub(target, fd, arena);
    }
}
