package com.chua.nativeturbojpeg.support;

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
import java.util.Objects;

/**
 * libjpeg-turbo 原生桥 —— 用 Panama FFM 下探 {@code chua_native_turbojpeg} 的扁平 C ABI。
 *
 * <p>动态库由 {@code utils-support-native-libjpeg-turbo} 提供，内部静态链接
 * libjpeg-turbo 3.1.2（含 SIMD 汇编核）。导出的六个符号：</p>
 * <ul>
 *   <li>{@code chua_tj_compress(src, w, h, pitch, pixfmt, quality, subsamp, flags, dst**, dstSize*)}</li>
 *   <li>{@code chua_tj_decompress(jpeg, size, pixfmt, flags, dst**, w*, h*, pitch*)}</li>
 *   <li>{@code chua_tj_probe(jpeg, size, w*, h*, subsamp*)}</li>
 *   <li>{@code chua_tj_free(ptr)} / {@code chua_tj_version()} / {@code chua_tj_last_error()}</li>
 * </ul>
 *
 * <p>加载失败时 {@link #isLoaded()} 返回 {@code false}，调用方（SPI 实现）据此降级，
 * 不会拖垮类加载。</p>
 *
 * <p>本类原位于 {@code utils-support-image-starter} 的
 * {@code com.chua.image.support.turbojpeg} 包，为让 FFM 绑定与动态库同住而迁入本模块。
 * 调用点：{@code TurboJpegImageDecoder} / {@code TurboJpegImageEncoder}。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class TurboJpegBridge {

    /**
     * 原生库基名。
     */
    private static final String LIBRARY_NAME = "chua_native_turbojpeg";

    /**
     * TurboJPEG 像素格式：24 位 RGB。
     */
    public static final int TJPF_RGB = 0;

    /**
     * TurboJPEG 像素格式：8 位灰度。
     */
    public static final int TJPF_GRAY = 6;

    /**
     * TurboJPEG 色度二次采样：4:2:0。
     */
    public static final int TJSAMP_420 = 2;

    /**
     * TurboJPEG 色度二次采样：单通道灰度。
     */
    public static final int TJSAMP_GRAY = 3;

    /**
     * 原生链接器。
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 库生命周期作用域，进程级共享。
     */
    private static final Arena ARENA = Arena.ofShared();

    /**
     * chua_tj_compress 句柄。
     */
    private static MethodHandle compressHandle;

    /**
     * chua_tj_decompress 句柄。
     */
    private static MethodHandle decompressHandle;

    /**
     * chua_tj_probe 句柄。
     */
    private static MethodHandle probeHandle;

    /**
     * chua_tj_free 句柄。
     */
    private static MethodHandle freeHandle;

    /**
     * chua_tj_version 句柄。
     */
    private static MethodHandle versionHandle;

    /**
     * chua_tj_last_error 句柄。
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
    private TurboJpegBridge() {
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
            compressHandle = downcall(lookup, "chua_tj_compress",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            decompressHandle = downcall(lookup, "chua_tj_decompress",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            probeHandle = downcall(lookup, "chua_tj_probe",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            freeHandle = downcall(lookup, "chua_tj_free",
                    FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
            versionHandle = downcall(lookup, "chua_tj_version",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            lastErrorHandle = downcall(lookup, "chua_tj_last_error",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            loaded = true;
            loadError = null;
            log.info("[TurboJpegBridge] libjpeg-turbo 原生库加载成功，内嵌版本 {}", version());
        } catch (Throwable e) {
            loadError = e;
            log.warn("[TurboJpegBridge] libjpeg-turbo 原生库不可用: {}", e.getMessage());
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
     * 读取内嵌的 libjpeg-turbo 版本号。
     *
     * @return 形如 {@code 3.1.2} 的版本串，未加载时返回空串
     */
    public static String version() {
        if (!loaded) {
            return "";
        }
        try {
            return readCString(((MemorySegment) versionHandle.invokeExact()).reinterpret(64));
        } catch (Throwable e) {
            return "";
        }
    }

    /**
     * 最近一次原生调用失败的原因描述。
     *
     * @return 错误描述，无错误时返回空串
     */
    public static String lastError() {
        if (!loaded) {
            return "turbojpeg native library not loaded";
        }
        try {
            return readCString(((MemorySegment) lastErrorHandle.invokeExact()).reinterpret(256));
        } catch (Throwable e) {
            return e.toString();
        }
    }

    /**
     * 探测 JPEG 头部，不解码像素。
     *
     * @param jpeg JPEG 字节
     * @return 长度为 3 的数组：{宽, 高, 色度二次采样}
     */
    public static int[] probe(byte[] jpeg) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment src = copyOf(arena, jpeg);
            MemorySegment width = arena.allocate(ValueLayout.JAVA_INT);
            MemorySegment height = arena.allocate(ValueLayout.JAVA_INT);
            MemorySegment subsamp = arena.allocate(ValueLayout.JAVA_INT);
            int ret = (int) probeHandle.invokeExact(src, (long) jpeg.length, width, height, subsamp);
            if (ret != 0) {
                throw new IllegalStateException("chua_tj_probe failed: " + lastError());
            }
            return new int[]{width.get(ValueLayout.JAVA_INT, 0), height.get(ValueLayout.JAVA_INT, 0),
                    subsamp.get(ValueLayout.JAVA_INT, 0)};
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("chua_tj_probe invocation failed", e);
        }
    }

    /**
     * 压缩为 JPEG。
     *
     * @param pixels      打包像素缓冲
     * @param width       宽
     * @param height      高
     * @param pitch       行跨距，0 表示按宽与像素格式自动计算
     * @param pixelFormat {@link #TJPF_RGB} / {@link #TJPF_GRAY}
     * @param quality     质量，1-100
     * @param subsamp     色度二次采样，{@link #TJSAMP_420} / {@link #TJSAMP_GRAY} 等
     * @param flags       TurboJPEG 标志位，透传给底层
     * @return JPEG 字节
     */
    public static byte[] compress(byte[] pixels, int width, int height, int pitch,
                                  int pixelFormat, int quality, int subsamp, int flags) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment src = copyOf(arena, pixels);
            MemorySegment dst = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment dstSize = arena.allocate(ValueLayout.JAVA_LONG);
            int ret = (int) compressHandle.invokeExact(src, width, height, pitch, pixelFormat,
                    quality, subsamp, flags, dst, dstSize);
            if (ret != 0) {
                throw new IllegalStateException("chua_tj_compress failed: " + lastError());
            }
            return take(dst, readCulong(dstSize));
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("chua_tj_compress invocation failed", e);
        }
    }

    /**
     * 解码为打包像素缓冲。
     *
     * @param jpeg        JPEG 字节
     * @param pixelFormat 目标像素格式
     * @param flags       TurboJPEG 标志位，透传给底层
     * @return 解码结果：像素（长度 {@code pitch * height}）、宽高与行跨距
     */
    public static Decoded decompress(byte[] jpeg, int pixelFormat, int flags) {
        requireLoaded();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment src = copyOf(arena, jpeg);
            MemorySegment dst = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment width = arena.allocate(ValueLayout.JAVA_INT);
            MemorySegment height = arena.allocate(ValueLayout.JAVA_INT);
            MemorySegment pitch = arena.allocate(ValueLayout.JAVA_INT);
            int ret = (int) decompressHandle.invokeExact(src, (long) jpeg.length, pixelFormat,
                    flags, dst, width, height, pitch);
            if (ret != 0) {
                throw new IllegalStateException("chua_tj_decompress failed: " + lastError());
            }
            int w = width.get(ValueLayout.JAVA_INT, 0);
            int h = height.get(ValueLayout.JAVA_INT, 0);
            int p = pitch.get(ValueLayout.JAVA_INT, 0);
            byte[] pixels = take(dst, (long) p * h);
            return new Decoded(pixels, w, h, p);
        } catch (RuntimeException e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException("chua_tj_decompress invocation failed", e);
        }
    }

    /**
     * 解码输出载体。
     *
     * @param pixels 打包像素字节，长度为 {@code pitch * height}
     * @param width  宽
     * @param height 高
     * @param pitch  行跨距，单位字节
     */
    public record Decoded(byte[] pixels, int width, int height, int pitch) {

        /**
         * 规范构造器：像素缓冲做防御性拷贝。
         *
         * <p>value class 前置条件——数组组件必须深不可变。唯一构造点
         * {@link #decompress(byte[], int, int)} 传入的 {@link #take(MemorySegment, long)}
         * 结果是从原生内存拷出的堆内数组（原生缓冲已释放），非调用方预分配回填的
         * 出参，故做整体拷贝。</p>
         */
        public Decoded {
            pixels = Objects.requireNonNull(pixels, "pixels 不能为 null").clone();
        }

        /**
         * 访问器覆写：返回像素缓冲的副本。
         *
         * @return 打包像素字节副本
         */
        @Override
        public byte[] pixels() {
            return pixels.clone();
        }
    }

    /**
     * 读出原生侧分配的缓冲区并立即释放。
     *
     * @param holder 存放指针的出参内存段
     * @param size   字节数
     * @return 堆内拷贝
     */
    private static byte[] take(MemorySegment holder, long size) throws Throwable {
        MemorySegment pointer = holder.get(ValueLayout.ADDRESS, 0).reinterpret(size);
        byte[] bytes = pointer.asSlice(0, size).toArray(ValueLayout.JAVA_BYTE);
        freeHandle.invokeExact(pointer);
        return bytes;
    }

    /**
     * 按平台读回 C 的 {@code unsigned long} 出参。
     *
     * <p>出参内存统一按 8 字节零初始化分配，Windows x64 的原生库只写低 4 字节，故这里按
     * 平台取对应宽度，不依赖"小端 + 高 4 字节恰为 0"这一巧合。</p>
     *
     * @param segment 出参内存
     * @return 原生写入的字节数
     */
    private static long readCulong(MemorySegment segment) {
        if ("windows".equals(NativeUtils.getOsPrefixName())) {
            return Integer.toUnsignedLong(segment.get(ValueLayout.JAVA_INT, 0));
        }
        return segment.get(ValueLayout.JAVA_LONG, 0);
    }

    /**
     * 校验原生库已加载。
     */
    private static void requireLoaded() {
        if (!loaded) {
            throw new IllegalStateException("libjpeg-turbo native library not loaded: "
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
        MemorySegment segment = arena.allocate(bytes.length == 0 ? 1 : bytes.length);
        MemorySegment.copy(bytes, 0, segment, ValueLayout.JAVA_BYTE, 0, bytes.length);
        return segment;
    }

    /**
     * 读取原生 C 字符串。
     *
     * @param segment 已按足够长度重解读的内存段
     * @return 字符串内容
     */
    private static String readCString(MemorySegment segment) {
        if (segment == null || segment.equals(MemorySegment.NULL)) {
            return "";
        }
        int length = 0;
        while (length < segment.byteSize() && segment.get(ValueLayout.JAVA_BYTE, length) != 0) {
            length++;
        }
        return new String(segment.asSlice(0, length).toArray(ValueLayout.JAVA_BYTE),
                StandardCharsets.UTF_8);
    }
}
