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
import java.util.Arrays;
import java.util.Comparator;

/**
 * 决定性区分「NTFS MFT 直读」与「walkdir 回退」的冒烟测试。
 *
 * <p>在扫描根目录下放置一个<b>深度 5</b> 的探针文件：walkdir 的 {@code WalkDir::max_depth(3)}
 * 永远看不到它，只有 MFT 直读能命中。因此同一个探针在两种引擎下的结果必然不同，
 * 可用来断言「本次构建/本次运行到底走了哪条路径」，而不是只看能否搜索。</p>
 *
 * <p>用法（需 JDK 25）：</p>
 * <pre>
 *   java --enable-native-access=ALL-UNNAMED FilesearchMftSmoke &lt;库路径&gt; &lt;扫描根目录&gt; mft|walkdir
 * </pre>
 *
 * <p>预期：Windows + 管理员 → {@code mft}（命中且带回正确 size）；
 * 其它平台或非管理员 → {@code walkdir}（不命中）。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class FilesearchMftSmoke {

    /**
     * 探针文件大小，故意取非整数 KB，便于校验 $DATA 的真实大小是否被正确回传
     */
    private static final int PROBE_BYTES = 4096 + 7;

    /**
     * 执行日志缓冲区
     */
    private static final StringBuilder LOG = new StringBuilder();

    /**
     * 工具类，禁止实例化。
     */
    private FilesearchMftSmoke() {
    }

    /**
     * 冒烟入口。
     *
     * @param argv [0]=动态库路径 [1]=扫描根目录 [2]=期望引擎（mft|walkdir）
     * @throws Throwable 任何失败
     */
    public static void main(String[] argv) throws Throwable {
        if (argv.length < 3) {
            System.err.println("用法: FilesearchMftSmoke <库路径> <扫描根目录> <mft|walkdir>");
            System.exit(2);
        }
        Path lib = Path.of(argv[0]);
        Path root = Path.of(argv[1]);
        String engine = argv[2];

        String tag = "mft-probe-" + System.nanoTime();
        Path probeRoot = root.resolve(tag + "-1");
        Path probe = probeRoot.resolve(tag + "-2").resolve(tag + "-3")
                .resolve(tag + "-4").resolve(tag + ".txt");
        Files.createDirectories(probe.getParent());
        byte[] payload = new byte[PROBE_BYTES];
        Arrays.fill(payload, (byte) 'x');
        Files.write(probe, payload);
        log("扫描根目录 = " + root);
        log("探针文件   = " + probe + "  (" + PROBE_BYTES + " 字节, 相对深度 5)");
        log("期望引擎   = " + engine);

        Linker linker = Linker.nativeLinker();
        try (Arena arena = Arena.ofShared()) {
            SymbolLookup lookup = SymbolLookup.libraryLookup(lib, arena);
            MethodHandle raw = linker.downcallHandle(
                    lookup.find("Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName")
                            .orElseThrow(() -> new AssertionError("缺少 __rawSearchByName")),
                    FunctionDescriptor.of(ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
            try (Arena call = Arena.ofConfined()) {
                MemorySegment rootSeg = call.allocateFrom(root.toString(), StandardCharsets.UTF_8);
                MemorySegment patSeg = call.allocateFrom(tag + ".txt", StandardCharsets.UTF_8);
                MemorySegment rp = (MemorySegment) raw.invokeExact(rootSeg, patSeg, 10);
                require(!rp.equals(MemorySegment.NULL), "__rawSearchByName 返回空指针");
                String json = rp.reinterpret(Long.MAX_VALUE).getString(0, StandardCharsets.UTF_8);
                int count = extractCount(json);
                log("返回 count = " + count);
                if ("mft".equals(engine)) {
                    require(count >= 1, "MFT 应命中深度 5 的探针文件，实际 " + count + "  " + json);
                    require(json.contains("\"size\":" + PROBE_BYTES),
                            "未回传正确大小（期望 " + PROBE_BYTES + "）: " + json);
                    require(json.contains(tag + ".txt"), "结果未包含探针文件名: " + json);
                } else {
                    require(count == 0,
                            "walkdir(max_depth=3) 不应命中深度 5 的探针文件，实际 " + count + "  " + json);
                }
            }
        }

        deleteQuietly(probeRoot);
        log("SMOKE OK");
        System.out.println(LOG);
    }

    /**
     * 断言。
     *
     * @param condition 条件
     * @param message   失败信息
     * @return 恒为 true
     */
    private static boolean require(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError("ASSERT FAIL " + message);
        }
        log("ASSERT ok   " + message);
        return true;
    }

    /**
     * 从 JSON envelope 中取 count。
     *
     * @param json JSON 文本
     * @return count 值
     */
    private static int extractCount(String json) {
        int i = json.indexOf("\"count\":");
        if (i < 0) {
            return -1;
        }
        int j = i + 8;
        while (j < json.length() && Character.isDigit(json.charAt(j))) {
            j++;
        }
        return Integer.parseInt(json.substring(i + 8, j));
    }

    /**
     * 尽力删除探针目录树。
     *
     * <p>入参必须是本次创建探针时用的<b>根目录</b>，而不是探针文件：
     * {@code Files.walk} 传入文件路径时只产出该文件自身，不会向上展开目录树；
     * 传文件或只传文件的父目录，都会把更上层的空目录留在磁盘根上。
     * 逆序遍历保证先删子、后删父。</p>
     *
     * @param probeRoot 本次创建的探针根目录
     */
    private static void deleteQuietly(Path probeRoot) {
        try {
            if (!Files.exists(probeRoot)) {
                return;
            }
            Files.walk(probeRoot)
                    .sorted(Comparator.reverseOrder())
                    .forEach(p -> {
                        try {
                            Files.deleteIfExists(p);
                        } catch (Exception ignored) {
                        }
                    });
        } catch (Exception ignored) {
        }
    }

    /**
     * 记录一行日志。
     *
     * @param line 日志内容
     */
    private static void log(String line) {
        LOG.append(line).append(System.lineSeparator());
    }
}
