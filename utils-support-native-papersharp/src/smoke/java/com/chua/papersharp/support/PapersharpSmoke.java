package com.chua.papersharp.support;

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
 * PaperSharp 原生库跨平台冒烟探针（自包含，无任何项目依赖）。
 *
 * <p>用法：{@code java --enable-native-access=ALL-UNNAMED PapersharpSmoke <native-lib-dir>}。
 * CI 在各平台构建出动态库后运行本探针，验证「加载 → 符号绑定 → 真实渲染」整条链路：
 * version 串、PDF（%PDF- 头）、PNG/JPEG（魔数）、SVG（&lt;svg 根）。</p>
 *
 * <p>与 {@code PaperSharpNativeBridge} 相同的加载顺序约定：依赖库
 * （libSkiaSharp / libHarfBuzzSharp）先入进程，主库 NativeHost 最后加载。</p>
 */
public final class PapersharpSmoke {

    private static final Linker LINKER = Linker.nativeLinker();

    private PapersharpSmoke() {
    }

    /**
     * 入口：arg0 为含动态库的目录（resources/native/&lt;platform&gt;）。
     *
     * @param args 命令行参数
     * @throws Throwable 任何失败都以非零退出
     */
    public static void main(String[] args) {
        try {
            if (args.length < 1) {
                fail("usage: PapersharpSmoke <native-lib-dir>");
            }
            run(Path.of(args[0]).toAbsolutePath());
        } catch (Throwable t) {
            t.printStackTrace();
            System.exit(1);
        }
    }

    private static void run(Path dir) throws Throwable {
        if (!Files.isDirectory(dir)) {
            fail("native dir not found: " + dir);
        }
        Path main = loadAllLibraries(dir);
        System.out.println("loaded main library: " + main.getFileName());
        try (Arena arena = Arena.ofConfined()) {
            SymbolLookup lookup = SymbolLookup.libraryLookup(main, arena);
            MethodHandle version = downcall(lookup, "papersharp_version",
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            MethodHandle renderPdf = downcall(lookup, "papersharp_render_pdf",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MethodHandle renderImage = downcall(lookup, "papersharp_render_image",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_FLOAT,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MethodHandle renderSvg = downcall(lookup, "papersharp_render_svg",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_FLOAT,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MethodHandle free = downcall(lookup, "papersharp_free",
                    FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

            String ver = readCString((MemorySegment) version.invoke(), 4096);
            check(!ver.isEmpty(), "version string not empty");
            System.out.println("[1] version = " + ver);

            String html = "<html><head><style>body{font-family:sans-serif;margin:24px}"
                    + "h1{color:#204e8a}td{padding:4px 8px}</style></head><body>"
                    + "<h1>黄月英 · PaperSharp</h1>"
                    + "<p>跨平台 AOT 冒烟：中文渲染 + 分页 PDF + 矢量图。</p>"
                    + "<table border=\"1\"><tr><td>项</td><td>结果</td></tr>"
                    + "<tr><td>native</td><td>ok</td></tr></table></body></html>";

            byte[] pdf = render(renderPdf, free, arena, html, "", 0);
            check(pdf.length > 4 && pdf[0] == '%' && pdf[1] == 'P' && pdf[2] == 'D' && pdf[3] == 'F',
                    "pdf magic, bytes=" + pdf.length);
            System.out.println("[2] pdf ok, bytes=" + pdf.length);

            byte[] png = render(renderImage, free, arena, html, "", 794f, 0, 100);
            check(png.length > 8 && (png[0] & 0xFF) == 0x89 && png[1] == 'P' && png[2] == 'N' && png[3] == 'G',
                    "png magic, bytes=" + png.length);
            System.out.println("[3] png ok, bytes=" + png.length);

            byte[] jpeg = render(renderImage, free, arena, html, "", 794f, 1, 90);
            check(jpeg.length > 3 && (jpeg[0] & 0xFF) == 0xFF && (jpeg[1] & 0xFF) == 0xD8,
                    "jpeg magic, bytes=" + jpeg.length);
            System.out.println("[4] jpeg ok, bytes=" + jpeg.length);

            byte[] svg = render(renderSvg, free, arena, html, "", 794f);
            String svgText = new String(svg, StandardCharsets.UTF_8);
            check(svgText.contains("<svg"), "svg root, chars=" + svgText.length());
            System.out.println("[5] svg ok, chars=" + svgText.length());

            System.out.println("==== SMOKE PASS ====");
        }
    }

    /**
     * 通用渲染调用：入参 [html, css, ...数值参数]，出参 (data, len, err) 三槽。
     */
    private static byte[] render(MethodHandle handle, MethodHandle free, Arena arena,
                                 String html, String css, Object... numeric) throws Throwable {
        MemorySegment htmlSeg = arena.allocateFrom(html, StandardCharsets.UTF_8);
        MemorySegment cssSeg = arena.allocateFrom(css, StandardCharsets.UTF_8);
        MemorySegment outData = arena.allocate(ValueLayout.ADDRESS);
        MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
        MemorySegment outErr = arena.allocate(ValueLayout.ADDRESS);
        Object[] call = new Object[2 + numeric.length + 3];
        call[0] = htmlSeg;
        call[1] = cssSeg;
        System.arraycopy(numeric, 0, call, 2, numeric.length);
        call[call.length - 3] = outData;
        call[call.length - 2] = outLen;
        call[call.length - 1] = outErr;
        int rc = (int) handle.asSpreader(Object[].class, call.length).invoke(call);
        if (rc != 0) {
            MemorySegment errPtr = outErr.get(ValueLayout.ADDRESS, 0);
            String msg = (errPtr == null || errPtr.address() == 0)
                    ? "unknown native error" : readCString(errPtr, 65536);
            fail("render failed: " + msg);
        }
        MemorySegment dataPtr = outData.get(ValueLayout.ADDRESS, 0);
        long len = outLen.get(ValueLayout.JAVA_LONG, 0);
        byte[] out = new byte[(int) len];
        MemorySegment.copy(dataPtr.reinterpret(len), 0, MemorySegment.ofArray(out), 0, len);
        free.invoke(dataPtr);
        return out;
    }

    /**
     * 按依赖顺序加载目录中的动态库：依赖库按名排序先加载，主库（NativeHost）最后加载。
     */
    private static Path loadAllLibraries(Path dir) throws Throwable {
        List<Path> dependencies = new ArrayList<>();
        Path main = null;
        try (Stream<Path> files = Files.list(dir)) {
            for (Path p : files.toList()) {
                String lower = p.getFileName().toString().toLowerCase();
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
            fail("main library (NativeHost) not found in " + dir);
        }
        dependencies.sort(Comparator.comparing(p -> p.getFileName().toString()));
        dependencies.add(main);
        for (Path p : dependencies) {
            System.out.println("  System.load " + p.getFileName());
            System.load(p.toAbsolutePath().toString());
        }
        return main;
    }

    private static MethodHandle downcall(SymbolLookup lookup, String symbol,
                                         FunctionDescriptor descriptor) throws Throwable {
        return LINKER.downcallHandle(
                lookup.find(symbol).orElseThrow(() -> new NoSuchMethodError(symbol)),
                descriptor);
    }

    /**
     * downcall 返回的指针段 byteSize 恒为 0，读取字符串前需 reinterpret 出可读边界。
     */
    private static String readCString(MemorySegment ptr, long max) {
        if (ptr == null || ptr.address() == 0) {
            return "";
        }
        return ptr.reinterpret(max).getString(0);
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            fail("ASSERT FAIL: " + what);
        }
        System.out.println("  assert ok: " + what);
    }

    private static void fail(String message) {
        System.err.println("==== SMOKE FAIL: " + message + " ====");
        System.exit(1);
    }
}
