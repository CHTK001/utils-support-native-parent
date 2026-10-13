package com.chua.nativeimageopt.support;

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
 * 图片压缩原生桥 —— 用 Panama FFM 下探 {@code chua_native_imageopt} 的扁平 C ABI。
 *
 * <p>动态库由 {@code utils-support-native-imageopt} 提供，内部静态链接两个上游 C 库：</p>
 * <ul>
 *   <li><b>libdeflate 1.26.1</b> —— zlib 压缩内核。同样压缩率下比 zlib 快 1.5~2 倍，
 *       且压缩级别做到 12（zlib 上限是 9）</li>
 *   <li><b>libimagequant 4.4.1</b> —— pngquant 的量化内核。在感知色彩空间做
 *       K-Means 迭代细化 + 有序抖动，同质量下比中位切分快两个数量级</li>
 * </ul>
 *
 * <h3>导出符号</h3>
 * <ul>
 *   <li>{@code chua_imageopt_version / chua_imageopt_deflate_version / chua_imageopt_quantize_version}</li>
 *   <li>{@code chua_imageopt_last_error}</li>
 *   <li>{@code chua_df_zlib_compress(src, len, level, dst, cap, len*)}</li>
 *   <li>{@code chua_df_zlib_compress_best(src, len, level, attempts, dst, cap, len*)}</li>
 *   <li>{@code chua_iq_quantize(rgba, n, w, h, colors, dither, speed, pal*, palCap, idx*, count*)}</li>
 * </ul>
 *
 * <h3>内存约定</h3>
 * <p>本库<b>不分配任何需要调用方释放的缓冲区</b>。所有输出缓冲都由 Java 侧预分配、
 * 按容量上限传入，因此不存在跨 FFM 边界的 free 归属歧义——这是刻意设计的，
 * 避免 native 内存泄漏在 Java 侧被静默引入。
 *
 * <p>加载失败时 {@link #isLoaded()} 返回 {@code false}，调用方（SPI 实现）据此降级到
 * 纯 Java 实现，不会拖垮类加载。
 *
 * @author CH
 * @since 4.0.0.43
 */
@Slf4j
public final class ImageOptBridge {

    /**
     * 原生库基名。
     */
    private static final String LIBRARY_NAME = "chua_native_imageopt";

    /**
     * 调色板最大颜色数，与原生侧 {@code MAX_PALETTE} 一致。
     */
    public static final int MAX_PALETTE_COLORS = 256;

    /**
     * 单个调色板颜色的字节数（RGBA）。
     */
    public static final int PALETTE_ENTRY_BYTES = 4;

    /**
     * 调色板缓冲区字节数。
     */
    public static final int PALETTE_BUFFER_BYTES = MAX_PALETTE_COLORS * PALETTE_ENTRY_BYTES;

    /**
     * 原生错误码：入参非法。
     */
    public static final int ERR_ARG = -1;

    /**
     * 原生错误码：输出缓冲区太小。
     */
    public static final int ERR_TOO_SMALL = -2;

    /**
     * 原生链接器。
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 库生命周期作用域，进程级共享。
     */
    private static final Arena ARENA = Arena.ofShared();

    /**
     * 读取固定长度上限，用于把零长原生指针重解读成可索引的内存段。
     */
    private static final long C_STRING_LIMIT = 256;

    /**
     * chua_df_zlib_compress 句柄。
     */
    private static MethodHandle compressHandle;

    /**
     * chua_df_zlib_compress_best 句柄。
     */
    private static MethodHandle compressBestHandle;

    /**
     * chua_iq_quantize 句柄。
     */
    private static MethodHandle quantizeHandle;

    /**
     * chua_imageopt_version 句柄。
     */
    private static MethodHandle versionHandle;

    /**
     * chua_imageopt_deflate_version 句柄。
     */
    private static MethodHandle deflateVersionHandle;

    /**
     * chua_imageopt_quantize_version 句柄。
     */
    private static MethodHandle quantizeVersionHandle;

    /**
     * chua_imageopt_last_error 句柄。
     */
    private static MethodHandle lastErrorHandle;

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
    private ImageOptBridge() {
    }

    /**
     * 提取并加载原生库，绑定下探句柄。重复调用安全。
     */
    public static synchronized void load() {
        if (loaded) {
            return;
        }
        try {
            Path targetDir = NativeUtils.tempRoot().resolve(LIBRARY_NAME);
            NativeLoader.of(LIBRARY_NAME)
                    .toTarget(targetDir)
                    // 不能用 LIBRARY_NAME + "*"：Unix 侧资源名为 lib<name>.{so,dylib}，
                    // 带 lib 前缀，锚定正则匹配不到，会导致 Linux/macOS 加载失败
                    .glob("*" + LIBRARY_NAME + "*")
                    .withMd5(true)
                    .extractOnly(false)
                    .load();
            Path libPath = targetDir.resolve(NativeUtils.getLibraryFileName(LIBRARY_NAME));
            if (!Files.exists(libPath)) {
                throw new IllegalStateException("native library not extracted: " + libPath);
            }
            SymbolLookup lookup = SymbolLookup.libraryLookup(libPath, ARENA);
            compressHandle = downcall(lookup, "chua_df_zlib_compress",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
            compressBestHandle = downcall(lookup, "chua_df_zlib_compress_best",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
            quantizeHandle = downcall(lookup, "chua_iq_quantize",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            versionHandle = downcall(lookup, "chua_imageopt_version",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            deflateVersionHandle = downcall(lookup, "chua_imageopt_deflate_version",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            quantizeVersionHandle = downcall(lookup, "chua_imageopt_quantize_version",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            lastErrorHandle = downcall(lookup, "chua_imageopt_last_error",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            loaded = true;
            loadError = null;
            log.info("[ImageOptBridge] 图片压缩原生库加载成功，{} / {}",
                    deflateVersion(), quantizeVersion());
        } catch (Throwable e) {
            loadError = e;
            log.warn("[ImageOptBridge] 图片压缩原生库不可用: {}", e.getMessage());
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
     * 读取门面版本串。
     *
     * @return 形如 {@code chua_native_imageopt/0.1.0 (libdeflate 1.26.1, libimagequant 4.4.1)}，
     *         未加载时返回空串
     */
    public static String version() {
        return readCString(invokeAddress(versionHandle));
    }

    /**
     * 读取内嵌的 libdeflate 版本号。
     *
     * @return 版本串，未加载时返回空串
     */
    public static String deflateVersion() {
        return readCString(invokeAddress(deflateVersionHandle));
    }

    /**
     * 读取内嵌的 libimagequant 版本号。
     *
     * @return 版本串，未加载时返回空串
     */
    public static String quantizeVersion() {
        return readCString(invokeAddress(quantizeVersionHandle));
    }

    /**
     * 最近一次原生调用失败的原因描述。
     *
     * @return 错误描述，未加载时返回降级说明
     */
    public static String lastError() {
        if (!loaded) {
            return "imageopt native library not loaded";
        }
        return readCString(invokeAddress(lastErrorHandle));
    }

    /**
     * 单次 zlib 压缩。
     *
     * @param data  待压缩数据
     * @param level 压缩级别 0~12
     * @return 带 zlib 头的压缩字节
     * @throws IllegalStateException 当原生库不可用或压缩失败时
     */
    public static byte[] zlibCompress(byte[] data, int level) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment src = copyOf(arena, data);
            // libdeflate 的 bound 对未压缩数据约为 input + input/1000 + 64
            int capacity = Math.max(64, data.length + data.length / 1000 + 64);
            MemorySegment dst = arena.allocate(capacity);
            MemorySegment length = arena.allocate(ValueLayout.JAVA_LONG);
            int ret = (int) compressHandle.invokeExact(src, (long) data.length, level,
                    dst, (long) capacity, length);
            long written = length.get(ValueLayout.JAVA_LONG, 0);
            if (ret == ERR_TOO_SMALL) {
                throw new IllegalStateException(
                        "chua_df_zlib_compress: output buffer too small, need " + written);
            }
            if (ret != 0) {
                throw new IllegalStateException("chua_df_zlib_compress failed: " + lastError());
            }
            return dst.asSlice(0, written).toArray(ValueLayout.JAVA_BYTE);
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("chua_df_zlib_compress invocation failed", e);
        }
    }

    /**
     * 多档试压取最小值的 zlib 压缩。
     *
     * @param data     待压缩数据
     * @param level    目标压缩级别 0~12
     * @param attempts 试压档数，1 表示不额外搜索
     * @return 带 zlib 头的最短压缩结果
     * @throws IllegalStateException 当原生库不可用或压缩失败时
     */
    public static byte[] zlibCompressBest(byte[] data, int level, int attempts) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment src = copyOf(arena, data);
            int capacity = Math.max(64, data.length + data.length / 1000 + 64);
            MemorySegment dst = arena.allocate(capacity);
            MemorySegment length = arena.allocate(ValueLayout.JAVA_LONG);
            int ret = (int) compressBestHandle.invokeExact(src, (long) data.length, level,
                    attempts, dst, (long) capacity, length);
            long written = length.get(ValueLayout.JAVA_LONG, 0);
            if (ret == ERR_TOO_SMALL) {
                throw new IllegalStateException(
                        "chua_df_zlib_compress_best: output buffer too small, need " + written);
            }
            if (ret != 0) {
                throw new IllegalStateException("chua_df_zlib_compress_best failed: " + lastError());
            }
            return dst.asSlice(0, written).toArray(ValueLayout.JAVA_BYTE);
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("chua_df_zlib_compress_best invocation failed", e);
        }
    }

    /**
     * libimagequant 调色板量化。
     *
     * @param rgba      每像素 4 字节 RGBA，长度为 {@code width * height * 4}
     * @param width     宽
     * @param height    高
     * @param maxColors 目标色数 2~256
     * @param dither    抖动强度 0~100
     * @param speed     libimagequant 速度档 1~10
     * @return 量化结果，含调色板与索引
     * @throws IllegalStateException 当原生库不可用或量化失败时
     */
    public static Quantized quantize(byte[] rgba, int width, int height, int maxColors,
                                     int dither, int speed) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment src = copyOf(arena, rgba);
            MemorySegment palette = arena.allocate(PALETTE_BUFFER_BYTES);
            MemorySegment indices = arena.allocate((long) width * height);
            MemorySegment colorCount = arena.allocate(ValueLayout.JAVA_INT);
            int ret = (int) quantizeHandle.invokeExact(src, (long) width * height,
                    width, height, maxColors, dither, speed,
                    palette, (long) PALETTE_BUFFER_BYTES, indices, colorCount);
            if (ret != 0) {
                throw new IllegalStateException("chua_iq_quantize failed: " + lastError());
            }
            int colors = colorCount.get(ValueLayout.JAVA_INT, 0);
            byte[] pal = palette.asSlice(0, (long) colors * PALETTE_ENTRY_BYTES)
                    .toArray(ValueLayout.JAVA_BYTE);
            byte[] idx = indices.toArray(ValueLayout.JAVA_BYTE);
            return new Quantized(pal, colors, idx);
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("chua_iq_quantize invocation failed", e);
        }
    }

    /**
     * 量化结果。
     *
     * @param palette 每色 4 字节 RGBA，长度为 {@code colors * 4}
     * @param colors  实际颜色数
     * @param indices 每像素一个索引，长度为 {@code width * height}
     */
    public record Quantized(byte[] palette, int colors, byte[] indices) {

        /**
         * 规范构造器：数组做防御性拷贝。
         */
        public Quantized {
            palette = palette == null ? new byte[0] : palette.clone();
            indices = indices == null ? new byte[0] : indices.clone();
        }

        /**
         * 访问器覆写：返回调色板副本。
         *
         * @return 调色板字节副本
         */
        @Override
        public byte[] palette() {
            return palette.clone();
        }

        /**
         * 访问器覆写：返回索引副本。
         *
         * @return 索引字节副本
         */
        @Override
        public byte[] indices() {
            return indices.clone();
        }
    }

    /**
     * 调用一个返回原生指针的零参句柄。
     *
     * @param handle 句柄，允许为 {@code null}（未加载场景）
     * @return 原生指针，未加载时返回 {@link MemorySegment#NULL}
     */
    private static MemorySegment invokeAddress(MethodHandle handle) {
        if (handle == null) {
            return MemorySegment.NULL;
        }
        try {
            return (MemorySegment) handle.invokeExact();
        } catch (Throwable e) {
            return MemorySegment.NULL;
        }
    }

    /**
     * 读取原生 C 字符串。
     *
     * <p>downcall 以 {@link ValueLayout#ADDRESS} 返回的是<b>零长</b> {@link MemorySegment}，
     * 不重解读出上界就无法按字节索引，必须先 {@code reinterpret}。
     *
     * @param segment 原生指针
     * @return 字符串内容；指针为空时返回空串
     */
    private static String readCString(MemorySegment segment) {
        if (segment == null || segment.equals(MemorySegment.NULL)) {
            return "";
        }
        MemorySegment bounded = segment.reinterpret(C_STRING_LIMIT);
        long length = 0;
        while (length < C_STRING_LIMIT && bounded.get(ValueLayout.JAVA_BYTE, length) != 0) {
            length++;
        }
        return new String(bounded.asSlice(0, length).toArray(ValueLayout.JAVA_BYTE),
                StandardCharsets.UTF_8);
    }

    /**
     * 校验原生库已加载。
     */
    private static void requireLoaded() {
        if (!loaded) {
            throw new IllegalStateException("imageopt native library not loaded: "
                    + (loadError == null ? "unknown reason" : loadError.getMessage()));
        }
    }

    /**
     * 绑定一个导出符号。
     *
     * @param lookup     符号查找器
     * @param symbol     符号名
     * @param descriptor 函数签名
     * @return 下探方法句柄
     */
    private static MethodHandle downcall(SymbolLookup lookup, String symbol, FunctionDescriptor descriptor) {
        return LINKER.downcallHandle(
                lookup.find(symbol).orElseThrow(() -> new UnsatisfiedLinkError(symbol + " not found")),
                descriptor);
    }

    /**
     * 把堆数组拷进给定作用域。
     *
     * @param arena 目标作用域
     * @param bytes 数据
     * @return 原生内存段
     */
    private static MemorySegment copyOf(Arena arena, byte[] bytes) {
        byte[] source = bytes == null ? new byte[0] : bytes;
        MemorySegment segment = arena.allocate(source.length == 0 ? 1 : source.length);
        MemorySegment.copy(source, 0, segment, ValueLayout.JAVA_BYTE, 0, source.length);
        return segment;
    }
}
