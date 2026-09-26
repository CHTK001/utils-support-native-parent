import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.file.Path;

/**
 * TurboJPEG 原生库往返验证示例。
 *
 * <p>本示例只使用 JDK Panama FFM，不依赖生产构件。它可以验证 {@code chua_native_turbojpeg}
 * 是否可加载、导出符号是否齐全，以及压缩、解析、解压是否能够完成像素往返。</p>
 *
 * <p>运行示例：{@code java --enable-native-access=ALL-UNNAMED
 * ChuaTurboJpegExample /path/to/libchua_native_turbojpeg.so}</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class ChuaTurboJpegExample {

    /**
     * TurboJPEG RGB 像素格式。
     */
    private static final int TJPF_RGB = 0;

    /**
     * TurboJPEG 4:2:0 色度采样格式。
     */
    private static final int TJSAMP_420 = 2;

    /**
     * 测试图宽度，故意使用非 16 对齐值。
     */
    private static final int WIDTH = 131;

    /**
     * 测试图高度，故意使用非 16 对齐值。
     */
    private static final int HEIGHT = 97;

    /**
     * Windows x64 的 C {@code unsigned long} 为 4 字节，其他常见平台为 8 字节。
     */
    private static final boolean WINDOWS =
            System.getProperty("os.name", "").toLowerCase().contains("windows");

    private static MethodHandle version;
    private static MethodHandle lastError;
    private static MethodHandle compress;
    private static MethodHandle probe;
    private static MethodHandle decompress;
    private static MethodHandle free;

    private ChuaTurboJpegExample() {
    }

    /**
     * 执行一次 TurboJPEG 原生库验证。
     *
     * @param args 第一个参数必须是原生库文件路径
     * @throws Throwable 原生符号绑定或验证失败时抛出
     */
    public static void main(String[] args) throws Throwable {
        if (args.length != 1) {
            System.out.println("usage: ChuaTurboJpegExample <native library path>");
            System.exit(2);
        }
        Path library = Path.of(args[0]);
        observe("platform", System.getProperty("os.name") + " / " + System.getProperty("os.arch")
                + " / culongBytes=" + (WINDOWS ? 4 : 8));
        try (Arena arena = Arena.ofShared()) {
            SymbolLookup lookup = SymbolLookup.libraryLookup(library.toAbsolutePath().toString(), arena);
            version = bind(lookup, "chua_tj_version", FunctionDescriptor.of(ValueLayout.ADDRESS));
            lastError = bind(lookup, "chua_tj_last_error", FunctionDescriptor.of(ValueLayout.ADDRESS));
            compress = bind(lookup, "chua_tj_compress",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            probe = bind(lookup, "chua_tj_probe",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            decompress = bind(lookup, "chua_tj_decompress",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS));
            free = bind(lookup, "chua_tj_free", FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
            observe("version", text((MemorySegment) version.invokeExact()));
            runRoundTrip(arena);
        }
        System.out.println("EXAMPLE PASS");
    }

    /**
     * 执行像素到 JPEG 再到像素的往返验证。
     *
     * @param arena 原生缓冲共享作用域
     * @throws Throwable 原生调用或断言失败时抛出
     */
    private static void runRoundTrip(Arena arena) throws Throwable {
        int pitch = WIDTH * 3;
        byte[] source = new byte[pitch * HEIGHT];
        for (int y = 0; y < HEIGHT; y++) {
            for (int x = 0; x < WIDTH; x++) {
                int base = y * pitch + x * 3;
                source[base] = (byte) (x * 255 / (WIDTH - 1));
                source[base + 1] = (byte) (y * 255 / (HEIGHT - 1));
                source[base + 2] = (byte) ((x + y) * 255 / (WIDTH + HEIGHT - 2));
            }
        }
        MemorySegment src = arena.allocateFrom(ValueLayout.JAVA_BYTE, source);
        MemorySegment jpegHolder = arena.allocate(ValueLayout.ADDRESS);
        MemorySegment sizeHolder = arena.allocate(ValueLayout.JAVA_LONG);
        int ret = (int) compress.invokeExact(src, WIDTH, HEIGHT, pitch, TJPF_RGB, 90, TJSAMP_420,
                0, jpegHolder, sizeHolder);
        check("compress", ret);
        long jpegSize = WINDOWS
                ? Integer.toUnsignedLong(sizeHolder.get(ValueLayout.JAVA_INT, 0))
                : sizeHolder.get(ValueLayout.JAVA_LONG, 0);
        MemorySegment jpeg = jpegHolder.get(ValueLayout.ADDRESS, 0).reinterpret(jpegSize);
        byte[] jpegBytes = jpeg.asSlice(0, jpegSize).toArray(ValueLayout.JAVA_BYTE);
        observe("jpegBytes", jpegSize + " SOI=" + hex(jpegBytes[0], jpegBytes[1])
                + " EOI=" + hex(jpegBytes[jpegBytes.length - 2], jpegBytes[jpegBytes.length - 1]));
        check("SOI marker", (jpegBytes[0] & 0xFF) == 0xFF && (jpegBytes[1] & 0xFF) == 0xD8 ? 0 : 1);
        check("EOI marker", (jpegBytes[jpegBytes.length - 2] & 0xFF) == 0xFF
                && (jpegBytes[jpegBytes.length - 1] & 0xFF) == 0xD9 ? 0 : 1);

        MemorySegment widthOut = arena.allocate(ValueLayout.JAVA_INT);
        MemorySegment heightOut = arena.allocate(ValueLayout.JAVA_INT);
        MemorySegment subsampOut = arena.allocate(ValueLayout.JAVA_INT);
        check("probe", (int) probe.invokeExact(jpeg, jpegSize, widthOut, heightOut, subsampOut));
        observe("probed", widthOut.get(ValueLayout.JAVA_INT, 0) + "x"
                + heightOut.get(ValueLayout.JAVA_INT, 0) + " subsamp=" + subsampOut.get(ValueLayout.JAVA_INT, 0));
        check("probed width", widthOut.get(ValueLayout.JAVA_INT, 0) == WIDTH ? 0 : 1);
        check("probed height", heightOut.get(ValueLayout.JAVA_INT, 0) == HEIGHT ? 0 : 1);
        free.invokeExact(jpegHolder.get(ValueLayout.ADDRESS, 0));

        MemorySegment dstHolder = arena.allocate(ValueLayout.ADDRESS);
        MemorySegment outW = arena.allocate(ValueLayout.JAVA_INT);
        MemorySegment outH = arena.allocate(ValueLayout.JAVA_INT);
        MemorySegment outPitch = arena.allocate(ValueLayout.JAVA_INT);
        MemorySegment copy = arena.allocate(jpegBytes.length);
        MemorySegment.copy(jpegBytes, 0, copy, ValueLayout.JAVA_BYTE, 0, jpegBytes.length);
        check("decompress", (int) decompress.invokeExact(copy, (long) jpegBytes.length,
                TJPF_RGB, 0, dstHolder, outW, outH, outPitch));
        int realW = outW.get(ValueLayout.JAVA_INT, 0);
        int realH = outH.get(ValueLayout.JAVA_INT, 0);
        int realPitch = outPitch.get(ValueLayout.JAVA_INT, 0);
        MemorySegment decoded = dstHolder.get(ValueLayout.ADDRESS, 0)
                .reinterpret((long) realPitch * realH);
        check("decompressed size", realW == WIDTH && realH == HEIGHT ? 0 : 1);
        long diff = 0;
        for (int y = 0; y < HEIGHT; y++) {
            for (int x = 0; x < WIDTH * 3; x++) {
                int a = source[y * pitch + x] & 0xFF;
                int b = decoded.get(ValueLayout.JAVA_BYTE, (long) y * realPitch + x) & 0xFF;
                diff += Math.abs(a - b);
            }
        }
        long mean = diff / (HEIGHT * pitch);
        observe("meanAbsDiff", mean + " outPitch=" + realPitch);
        check("pixels close", mean <= 12 ? 0 : 1);
        free.invokeExact(dstHolder.get(ValueLayout.ADDRESS, 0));
    }

    /**
     * 绑定一个必需的原生导出符号。
     *
     * @param lookup 符号查找器
     * @param symbol 符号名
     * @param descriptor 函数描述符
     * @return 原生方法句柄
     */
    private static MethodHandle bind(SymbolLookup lookup, String symbol, FunctionDescriptor descriptor) {
        MemorySegment address = lookup.find(symbol).orElseThrow(
                () -> new IllegalStateException("missing exported symbol " + symbol));
        return Linker.nativeLinker().downcallHandle(address, descriptor);
    }

    /**
     * 读取原生返回的 UTF-8 字符串。
     *
     * @param pointer 字符指针
     * @return 字符串内容
     */
    private static String text(MemorySegment pointer) {
        return pointer.reinterpret(256).getString(0);
    }

    /**
     * 断言观测值为 0。
     *
     * @param name 断言名称
     * @param value 非 0 即失败
     */
    private static void check(String name, long value) {
        if (value == 0) {
            System.out.println("ASSERT ok   " + name);
            return;
        }
        System.out.println("ASSERT FAIL " + name + " (value=" + value + ")");
        System.exit(1);
    }

    /**
     * 输出观测值。
     *
     * @param name 名称
     * @param value 内容
     */
    private static void observe(String name, Object value) {
        System.out.println("OBSERVE " + name + "=" + value);
    }

    /**
     * 格式化两个字节为十六进制字符串。
     *
     * @param hi 高字节
     * @param lo 低字节
     * @return 形如 FFD8 的字符串
     */
    private static String hex(int hi, int lo) {
        return String.format("%02X%02X", hi & 0xFF, lo & 0xFF);
    }
}
