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
 * file_search 原生库冒烟测试：只用 JDK Panama FFM，不依赖项目任何模块。
 *
 * <p>用法：{@code java --enable-native-access=ALL-UNNAMED FilesearchFlatAbiSmoke <库路径>}</p>
 *
 * <p>之所以直接用 FFM 调扁平 C ABI 而不是走 Java_... JNI 层：该库的 JNI 层不符合
 * JNI 约定（缺少 JNIEnv 与 jclass 前导参数，getVersion 以 C 字符串指针冒充
 * jstring），调用会让 JVM 崩溃；仓库中的 Java 绑定也早已改走这套 C ABI。</p>
 */
public final class FilesearchFlatAbiSmoke {

    private static final StringBuilder LOG = new StringBuilder();

    public static void main(String[] argv) throws Throwable {
        if (argv.length == 0) {
            System.err.println("用法: FilesearchFlatAbiSmoke <库路径>");
            System.exit(2);
        }
        Path lib = Path.of(argv[0]);
        require(Files.exists(lib), "库文件不存在: " + lib);
        log("库 = " + lib + "  (" + Files.size(lib) + " 字节)");

        Path tmp = Files.createTempDirectory("fs-smoke");
        for (int i = 1; i <= 3; i++) {
            Files.writeString(tmp.resolve("s" + i + ".log"), "x", StandardCharsets.UTF_8);
        }
        Files.writeString(tmp.resolve("other.txt"), "y", StandardCharsets.UTF_8);
        log("临时目录 = " + tmp);

        Linker linker = Linker.nativeLinker();
        try (Arena arena = Arena.ofShared()) {
            SymbolLookup lookup = SymbolLookup.libraryLookup(lib, arena);

            MethodHandle version = linker.downcallHandle(
                    lookup.find("fast_get_version")
                            .orElseThrow(() -> new AssertionError("缺少 fast_get_version")),
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
            MemorySegment vp = (MemorySegment) version.invokeExact();
            require(!vp.equals(MemorySegment.NULL), "fast_get_version 返回空指针");
            String ver = vp.reinterpret(64).getString(0, StandardCharsets.UTF_8);
            require(ver != null && !ver.isEmpty(), "版本串为空");
            log("fast_get_version = " + ver);

            MethodHandle raw = linker.downcallHandle(
                    lookup.find("Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName")
                            .orElseThrow(() -> new AssertionError("缺少 __rawSearchByName")),
                    FunctionDescriptor.of(ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
            try (Arena call = Arena.ofConfined()) {
                MemorySegment root = call.allocateFrom(tmp.toString(), StandardCharsets.UTF_8);
                MemorySegment pat = call.allocateFrom("*.log", StandardCharsets.UTF_8);
                MemorySegment rp = (MemorySegment) raw.invokeExact(root, pat, 100);
                require(!rp.equals(MemorySegment.NULL), "__rawSearchByName 返回空指针");
                String json = rp.reinterpret(Long.MAX_VALUE).getString(0, StandardCharsets.UTF_8);
                log("__rawSearchByName = " + json);
                require(json.contains("\"count\":3"), "预期命中 3 个 .log，实际: " + json);
                require(json.contains("s1.log"), "结果未包含 s1.log");
            }
        }

        log("SMOKE OK");
        System.out.println(LOG);
    }

    private static boolean require(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError("ASSERT FAIL " + message);
        }
        log("ASSERT ok   " + message);
        return true;
    }

    private static void log(String line) {
        LOG.append(line).append(System.lineSeparator());
    }
}
