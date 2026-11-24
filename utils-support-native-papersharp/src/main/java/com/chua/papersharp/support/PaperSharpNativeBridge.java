package com.chua.papersharp.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import lombok.extern.slf4j.Slf4j;

import java.io.IOException;
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
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.stream.Stream;

/**
 * PaperSharp 原生桥 —— 用 Panama FFM 下探 AOT 动态库 {@code NativeHost} 的扁平 C ABI。
 *
 * <p>动态库由 {@code src/main/native/NativeHost}（.NET 10 Native AOT 工程）构建，
 * 内嵌 <a href="https://github.com/SpringHgui/papersharp">PaperSharp</a> 1.0.5 渲染引擎
 * （HTML+CSS → 分页 PDF / 图片，中文排版、跨页表格、@page 页眉页脚），
 * 以及随包分发的 {@code libSkiaSharp} / {@code libHarfBuzzSharp}。</p>
 *
 * <h3>导出符号</h3>
 * <ul>
 *   <li>{@code papersharp_version}</li>
 *   <li>{@code papersharp_render_pdf(html*, css*, int pageSize, data**, len*, err**)}</li>
 *   <li>{@code papersharp_render_image(html*, css*, float width, int format, int quality, data**, len*, err**)}</li>
 *   <li>{@code papersharp_render_svg(html*, css*, float width, data**, len*, err**)}</li>
 *   <li>{@code papersharp_render_ico(html*, css*, int size, float width, data**, len*, err**)}</li>
 *   <li>{@code papersharp_free(ptr)}</li>
 * </ul>
 *
 * <h3>内存约定</h3>
 * <p>输出字节块与错误串由 native 侧（Marshal.AllocHGlobal）分配，Java 侧读取后必须
 * 通过 {@code papersharp_free} 归还——输出尺寸事先未知，无法像 imageopt 那样由 Java 预分配。</p>
 *
 * <h3>加载顺序</h3>
 * <p>{@link NativeLoader} 只负责把 {@code native/<platform>/} 下的动态库提取到临时目录
 * （{@code extractOnly(true)}）；随后由 {@code loadAllLibraries} 显式按依赖顺序
 * {@code System.load}：{@code libHarfBuzzSharp} / {@code libSkiaSharp} 等依赖库先加载，
 * 主库 {@code NativeHost} 最后加载——Windows LoadLibrary 的依赖搜索不含被加载库
 * 自身所在目录，依赖库必须先进入进程模块表。</p>
 *
 * <p>加载失败时 {@link #isLoaded()} 返回 {@code false}，调用方（SPI 实现）据此快速失败，
 * 不会拖垮类加载。</p>
 *
 * @author CH
 * @since 4.0.0.43
 */
@Slf4j
public final class PaperSharpNativeBridge {

    /**
     * 提取目录 / 加载任务名。
     */
    private static final String TASK_ID = "papersharp-native";

    /**
     * 渲染页面规格：A4。
     */
    public static final int PAGE_SIZE_A4 = 0;

    /**
     * 渲染页面规格：US Letter。
     */
    public static final int PAGE_SIZE_LETTER = 1;

    /**
     * 图片编码：PNG。
     */
    public static final int IMG_PNG = 0;

    /**
     * 图片编码：JPEG。
     */
    public static final int IMG_JPEG = 1;

    /**
     * 图片编码：WebP。
     */
    public static final int IMG_WEBP = 2;

    /**
     * 原生链接器。
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 库生命周期作用域，进程级共享。
     */
    private static final Arena ARENA = Arena.ofShared();

    private static MethodHandle renderPdfHandle;
    private static MethodHandle renderImageHandle;
    private static MethodHandle renderSvgHandle;
    private static MethodHandle renderIcoHandle;
    private static MethodHandle freeHandle;
    private static MethodHandle versionHandle;

    /**
     * 加载状态。
     */
    private static volatile boolean loaded;

    /**
     * 加载失败原因。
     */
    private static volatile Throwable loadError;

    static {
        load();
    }

    /**
     * 工具类，禁止实例化。
     */
    private PaperSharpNativeBridge() {
    }

    /**
     * 提取并加载原生库，绑定下探句柄。重复调用安全。
     */
    public static synchronized void load() {
        if (loaded) {
            return;
        }
        try {
            Path targetDir = NativeUtils.tempRoot().resolve(TASK_ID);
            // 只提取不加载：主库必须最后加载（Windows LoadLibrary 的依赖搜索
            // 不含被加载库自身目录，libSkiaSharp / libHarfBuzzSharp 须先入进程模块表）
            NativeLoader.of(TASK_ID)
                    .toTarget(targetDir)
                    .glob("*")
                    .withMd5(true)
                    .extractOnly(true)
                    .load();
            Path libPath = loadAllLibraries(targetDir);
            SymbolLookup lookup = SymbolLookup.libraryLookup(libPath, ARENA);
            renderPdfHandle = downcall(lookup, "papersharp_render_pdf",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            renderImageHandle = downcall(lookup, "papersharp_render_image",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_FLOAT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            renderSvgHandle = downcall(lookup, "papersharp_render_svg",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_FLOAT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            renderIcoHandle = downcall(lookup, "papersharp_render_ico",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_FLOAT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            freeHandle = downcall(lookup, "papersharp_free",
                    FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
            versionHandle = downcall(lookup, "papersharp_version",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            loaded = true;
            loadError = null;
            log.info("[PaperSharpNativeBridge] PaperSharp 原生渲染库加载成功: {}", version());
        } catch (Throwable e) {
            loadError = e;
            log.warn("[PaperSharpNativeBridge] PaperSharp 原生渲染库不可用: {}", e.getMessage());
        }
    }

    /**
     * 原生库是否可用。
     *
     * @return 可用返回 {@code true}
     */
    public static boolean isLoaded() {
        return loaded;
    }

    /**
     * 原生库加载失败原因。
     *
     * @return 失败异常，加载成功返回 {@code null}
     */
    public static Throwable getLoadError() {
        return loadError;
    }

    /**
     * 引擎版本串。
     *
     * @return 形如 {@code papersharp-native/1.0.0 (PaperSharp 1.0.5, net10.0 AOT)}，未加载返回空串
     */
    public static String version() {
        if (!loaded) {
            return "";
        }
        try {
            return readCString((MemorySegment) versionHandle.invoke());
        } catch (Throwable e) {
            return "";
        }
    }

    /**
     * 渲染分页 PDF。
     *
     * @param html     HTML 文档（可含内联 style）
     * @param css      附加 CSS（可为空串；@page 等分页规则写在 HTML 内即可）
     * @param pageSize {@link #PAGE_SIZE_A4} 或 {@link #PAGE_SIZE_LETTER}
     * @return PDF 字节
     */
    public static byte[] renderPdf(String html, String css, int pageSize) {
        return invokeRender(renderPdfHandle, html, css, new Object[]{pageSize});
    }

    /**
     * 渲染整页长图。
     *
     * @param html    HTML 文档
     * @param css     附加 CSS
     * @param width   布局视口宽度（CSS px）
     * @param format  {@link #IMG_PNG} / {@link #IMG_JPEG} / {@link #IMG_WEBP}
     * @param quality 编码质量 1~100（PNG 忽略）
     * @return 图片字节
     */
    public static byte[] renderImage(String html, String css, float width, int format, int quality) {
        return invokeRender(renderImageHandle, html, css, new Object[]{width, format, quality});
    }

    /**
     * 渲染 SVG（矢量整页图）。
     *
     * @param html  HTML 文档
     * @param css   附加 CSS
     * @param width 布局视口宽度（CSS px）
     * @return SVG 字节
     */
    public static byte[] renderSvg(String html, String css, float width) {
        return invokeRender(renderSvgHandle, html, css, new Object[]{width});
    }

    /**
     * 渲染 ICO 图标。
     *
     * @param html  HTML 文档
     * @param css   附加 CSS
     * @param size  ICO 条目边长（px，常用 16/32/48/256）
     * @param width 布局视口宽度（CSS px）
     * @return ICO 字节
     */
    public static byte[] renderIco(String html, String css, int size, float width) {
        return invokeRender(renderIcoHandle, html, css, new Object[]{size, width});
    }

    /**
     * 通用渲染调用：入参为 [html, css, ...数值参数]，出参为 (data, len, err) 三槽。
     * 在受限 Arena 里编组 UTF-8 字符串与出参槽，读回字节块后归还 native 内存。
     */
    private static byte[] invokeRender(MethodHandle handle, String html, String css, Object[] numericArgs) {
        if (!loaded) {
            throw new IllegalStateException("PaperSharp 原生库未加载: "
                    + (loadError == null ? "not attempted" : loadError.getMessage()));
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment htmlSeg = arena.allocateFrom(html, StandardCharsets.UTF_8);
            MemorySegment cssSeg = arena.allocateFrom(css, StandardCharsets.UTF_8);
            MemorySegment outData = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment outErr = arena.allocate(ValueLayout.ADDRESS);

            Object[] callArgs = new Object[2 + numericArgs.length + 3];
            callArgs[0] = htmlSeg;
            callArgs[1] = cssSeg;
            System.arraycopy(numericArgs, 0, callArgs, 2, numericArgs.length);
            callArgs[callArgs.length - 3] = outData;
            callArgs[callArgs.length - 2] = outLen;
            callArgs[callArgs.length - 1] = outErr;
            int rc = (int) handle.asSpreader(Object[].class, callArgs.length).invoke(callArgs);
            if (rc != 0) {
                MemorySegment errPtr = outErr.get(ValueLayout.ADDRESS, 0);
                String msg = (errPtr == null || errPtr.address() == 0)
                        ? "unknown native error"
                        : readCString(errPtr);
                if (errPtr != null && errPtr.address() != 0) {
                    freeHandle.invoke(errPtr);
                }
                throw new IllegalStateException("PaperSharp 渲染失败: " + msg);
            }
            MemorySegment dataPtr = outData.get(ValueLayout.ADDRESS, 0);
            long len = outLen.get(ValueLayout.JAVA_LONG, 0);
            byte[] out = new byte[(int) len];
            MemorySegment.copy(dataPtr.reinterpret(len), 0, MemorySegment.ofArray(out), 0, len);
            freeHandle.invoke(dataPtr);
            return out;
        } catch (IllegalStateException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("PaperSharp 原生调用失败", e);
        }
    }

    /**
     * 按依赖顺序加载提取目录中的全部动态库：依赖库（libSkiaSharp / libHarfBuzzSharp 等）
     * 按文件名排序先加载，主库（NativeHost）最后加载。
     *
     * @param dir 提取目录
     * @return 主库路径
     * @throws IOException 目录列举失败
     */
    private static Path loadAllLibraries(Path dir) throws IOException {
        List<Path> dependencies = new ArrayList<>();
        Path main = null;
        try (Stream<Path> files = Files.list(dir)) {
            for (Path p : files.toList()) {
                String name = p.getFileName().toString();
                String lower = name.toLowerCase();
                if (!(lower.endsWith(".dll") || lower.endsWith(".so") || lower.endsWith(".dylib"))) {
                    continue;
                }
                if (lower.startsWith("nativehost.") || lower.startsWith("libnativehost.")) {
                    main = p;
                } else {
                    dependencies.add(p);
                }
            }
        }
        if (main == null) {
            throw new IllegalStateException("native library not extracted in " + dir);
        }
        dependencies.sort(Comparator.comparing(p -> p.getFileName().toString()));
        dependencies.add(main);
        for (Path p : dependencies) {
            System.load(p.toAbsolutePath().toString());
        }
        return main;
    }

    /**
     * 绑定下探句柄，找不到符号时直接失败。
     */
    private static MethodHandle downcall(SymbolLookup lookup, String symbol, FunctionDescriptor descriptor)
            throws Throwable {
        return LINKER.downcallHandle(
                lookup.find(symbol).orElseThrow(() -> new NoSuchMethodError(symbol)),
                descriptor);
    }

    /**
     * downcall 返回的指针段 byteSize 恒为 0，读取字符串前需 reinterpret 出可读边界。
     */
    private static final long MAX_STRING = 1 << 20;

    /**
     * 读取 native 侧 \0 结尾的 UTF-8 字符串。
     */
    private static String readCString(MemorySegment ptr) {
        if (ptr == null || ptr.address() == 0) {
            return "";
        }
        return ptr.reinterpret(MAX_STRING).getString(0);
    }
}
