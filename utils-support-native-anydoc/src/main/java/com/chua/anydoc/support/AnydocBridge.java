package com.chua.anydoc.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.Objects;
import lombok.extern.slf4j.Slf4j;

/**
 * {@code anydoc_ffi} 原生库的 FFM 绑定与唯一 Java 门面。
 *
 * <p>底层是 Rust cdylib（源码见本模块 {@code src/main/rust/}），基于
 * firecrawl {@code anydoc 0.2.5} 把 14 种文档（doc/docx/odt/pdf/ppt/pptx/
 * rtf/epub/xlsx/xlsm/xlsb/xls/ods/odp/csv）解析为 GitHub-Flavored Markdown。
 * 纯文档解析：无 OCR、无模型推理；扫描件 PDF 会返回 {@code NeedsOcr} 类错误。</p>
 *
 * <p><b>加载方式</b>：统一走项目受控的 {@link NativeLoader}，按平台目录
 * （{@code resources/native/<os>-<arch>/}）抽取并做 MD5 去重，避免业务代码
 * 直接调用 {@code System.load}。平台产物由 {@code native-anydoc} workflow
 * 四平台交叉编译产出。</p>
 *
 * <p><b>C-ABI 约定</b>：convert 类导出成功返回堆分配的 NUL 结尾 UTF-8 C 串
 * （必须用 {@code anydoc_free_string} 释放），失败返回 NULL 并可经
 * {@code anydoc_last_error()} 取本线程错误信息；{@code anydoc_version} 与
 * {@code anydoc_last_error} 返回的是静态/线程局部存储，<b>禁止 free</b>。
 * 本类负责全部内存管理，调用方只接触 Java {@code String}。</p>
 *
 * <p><b>线程安全</b>：加载一次性（双重检查锁）；原生库本身无全局可变状态，
 * 错误信息按线程隔离，可安全并发调用。</p>
 *
 * @author CH
 * @since 4.0.0.43
 */
@Slf4j
public final class AnydocBridge {

    /**
     * 原生库逻辑名，与 {@code NativeLoader} 的 taskId 对应。
     */
    private static final String TASK_ID = "anydoc-ffi";

    /**
     * 动态库文件名主干（不含 lib 前缀与平台后缀）。
     */
    private static final String LIB_STEM = "anydoc_ffi";

    /**
     * FFM 链接器。
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * convert 返回串的 reinterpret 读取上限。仅作为越界检查边界，
     * 不发生实际分配；{@code getString} 遇 NUL 即停。
     */
    private static final long MAX_OUTPUT_BYTES = 1L << 32;

    /**
     * 已加载库的符号查找器，加载成功后赋值。
     */
    private static volatile SymbolLookup symLookup;

    /**
     * 是否已尝试过加载（无论成败）。
     */
    private static volatile boolean resolved;

    /**
     * 加载是否成功。
     */
    private static volatile boolean available;

    /**
     * {@code anydoc_version} 句柄。
     */
    private static volatile MethodHandle versionHandle;

    /**
     * {@code anydoc_last_error} 句柄。
     */
    private static volatile MethodHandle lastErrorHandle;

    /**
     * {@code anydoc_free_string} 句柄。
     */
    private static volatile MethodHandle freeStringHandle;

    /**
     * {@code anydoc_convert_file} 句柄。
     */
    private static volatile MethodHandle convertFileHandle;

    /**
     * {@code anydoc_convert_bytes} 句柄。
     */
    private static volatile MethodHandle convertBytesHandle;

    /**
     * 工具类，禁止实例化。
     */
    private AnydocBridge() {
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
        synchronized (AnydocBridge.class) {
            if (resolved) {
                return available;
            }
            try {
                NativeLoader.of(TASK_ID)
                        .glob(NativeUtils.getLibraryFileName(LIB_STEM))
                        .toTarget(NativeUtils.tempRoot().resolve(TASK_ID).toFile().getAbsolutePath())
                        .load();
                symLookup = SymbolLookup.loaderLookup();
                versionHandle = bind("anydoc_version",
                        FunctionDescriptor.of(ValueLayout.ADDRESS));
                lastErrorHandle = bind("anydoc_last_error",
                        FunctionDescriptor.of(ValueLayout.ADDRESS));
                freeStringHandle = bind("anydoc_free_string",
                        FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
                convertFileHandle = bind("anydoc_convert_file",
                        FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
                convertBytesHandle = bind("anydoc_convert_bytes",
                        FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                                ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
                available = true;
                log.debug("anydoc_ffi 原生库已加载并完成 5 个导出绑定");
            } catch (Throwable e) {
                available = false;
                log.warn("anydoc 文档解析原生库不可用，文档转 Markdown 能力降级: {}", e.toString());
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
     * 原生库与封装层版本信息，如 {@code anydoc-ffi 0.1.0 (anydoc 0.2.5)}。
     *
     * @return 版本串
     * @throws IllegalStateException 原生库不可用或调用失败时
     */
    public static String version() {
        requireAvailable();
        try {
            MemorySegment v = (MemorySegment) versionHandle.invoke();
            return v.reinterpret(MAX_OUTPUT_BYTES).getString(0, StandardCharsets.UTF_8);
        } catch (Throwable e) {
            throw new IllegalStateException("anydoc_version 调用失败", e);
        }
    }

    /**
     * 把一个文档文件转换为 Markdown。
     *
     * <p>格式按内容签名探测，文件扩展名兜底；无内容签名的格式（如 CSV）
     * 依赖扩展名识别。</p>
     *
     * @param file 文档路径，不允许为 null
     * @return GitHub-Flavored Markdown 文本
     * @throws IllegalStateException 原生库不可用、路径不存在或解析失败时；
     *         异常消息携带原生侧真实错误
     */
    public static String convertFile(Path file) {
        requireAvailable();
        Objects.requireNonNull(file, "file");
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment path = arena.allocateFrom(
                    file.toAbsolutePath().toString(), StandardCharsets.UTF_8);
            MemorySegment out = (MemorySegment) convertFileHandle.invoke(path);
            return takeOutput(out);
        } catch (IllegalStateException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("anydoc_convert_file 调用失败", e);
        }
    }

    /**
     * 把内存中的文档字节转换为 Markdown。
     *
     * @param data   文档原始字节，不允许为 null 或空
     * @param format 格式扩展名（如 {@code "csv"}）；传 null 表示按内容签名
     *               自动探测。CSV 等无签名格式必须显式命名
     * @return GitHub-Flavored Markdown 文本
     * @throws IllegalStateException 原生库不可用或解析失败时；
     *         异常消息携带原生侧真实错误
     */
    public static String convertBytes(byte[] data, String format) {
        requireAvailable();
        Objects.requireNonNull(data, "data");
        if (data.length == 0) {
            throw new IllegalArgumentException("data 为空");
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment in = arena.allocate(data.length, 1);
            MemorySegment.copy(data, 0, in, ValueLayout.JAVA_BYTE, 0, data.length);
            MemorySegment fmt = format == null
                    ? MemorySegment.NULL
                    : arena.allocateFrom(format, StandardCharsets.UTF_8);
            MemorySegment out = (MemorySegment) convertBytesHandle.invoke(
                    in, (long) data.length, fmt);
            return takeOutput(out);
        } catch (IllegalStateException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("anydoc_convert_bytes 调用失败", e);
        }
    }

    /**
     * 当前线程最近一次原生侧错误信息。
     *
     * @return 错误信息；无错误或原生库不可用时返回 null。
     *         下一次 anydoc 原生调用前有效
     */
    public static String lastError() {
        if (!load() || lastErrorHandle == null) {
            return null;
        }
        try {
            MemorySegment err = (MemorySegment) lastErrorHandle.invoke();
            if (err == null || err.equals(MemorySegment.NULL)) {
                return null;
            }
            return err.reinterpret(MAX_OUTPUT_BYTES).getString(0, StandardCharsets.UTF_8);
        } catch (Throwable e) {
            return null;
        }
    }

    /**
     * 读取 convert 返回串并释放原生内存。
     *
     * @param out convert 导出返回的指针段
     * @return Markdown 文本
     * @throws Throwable 读取或释放失败时；NULL 输入抛带原生错误信息的
     *         {@link IllegalStateException}
     */
    private static String takeOutput(MemorySegment out) throws Throwable {
        if (out == null || out.equals(MemorySegment.NULL)) {
            String err = lastError();
            throw new IllegalStateException("anydoc 转换失败: " + (err == null ? "未知错误" : err));
        }
        String markdown = out.reinterpret(MAX_OUTPUT_BYTES).getString(0, StandardCharsets.UTF_8);
        freeStringHandle.invoke(out);
        return markdown;
    }

    /**
     * 断言原生库可用。
     *
     * @throws IllegalStateException 不可用时
     */
    private static void requireAvailable() {
        if (!load()) {
            throw new IllegalStateException("anydoc_ffi 原生库不可用（见启动日志）");
        }
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
}
