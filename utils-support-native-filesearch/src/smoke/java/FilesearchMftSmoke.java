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
 * <p>放置两个<b>同名</b>探针文件，只有目录深度不同：</p>
 * <ul>
 *   <li>深度 1：两条引擎都必须命中；</li>
 *   <li>深度 5：walkdir 的 {@code WalkDir::max_depth(3)} 永远看不到，只有 MFT 直读能命中。</li>
 * </ul>
 *
 * <p>因此同一次搜索在 MFT 下应返回 <b>2</b> 条、在 walkdir 下应返回 <b>1</b> 条，
 * 差值即本次实际走的是哪条路径，不依赖「能否搜索」这种弱判据。</p>
 *
 * <p>深度 1 探针是<b>与引擎无关的正向断言</b>：少了它，walkdir 分支只剩
 * {@code count == 0} 一条断言，一个「永远返回空」「size 恒为 0」的构建照样全绿。
 * 它同时校验 size 回传，等于在每个平台上都断言了「真的检索到了、且元数据正确」。</p>
 *
 * <p>用法（需 JDK 25）：</p>
 * <pre>
 *   java --enable-native-access=ALL-UNNAMED FilesearchMftSmoke &lt;库路径&gt; &lt;扫描根目录&gt; mft|walkdir
 * </pre>
 *
 * <p>预期：Windows + 管理员 → {@code mft}（count=2）；
 * 其它平台或非管理员 → {@code walkdir}（count=1）。</p>
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
        // 两个探针同名，只差目录深度：同一次搜索即可同时验「能不能搜到」和「走的是哪条路」
        Path flatProbe = root.resolve(tag + ".txt");
        Path deepRoot = root.resolve(tag + "-1");
        Path deepProbe = deepRoot.resolve(tag + "-2").resolve(tag + "-3")
                .resolve(tag + "-4").resolve(tag + ".txt");
        Files.createDirectories(deepProbe.getParent());
        byte[] payload = new byte[PROBE_BYTES];
        Arrays.fill(payload, (byte) 'x');
        Files.write(flatProbe, payload);
        Files.write(deepProbe, payload);
        log("扫描根目录 = " + root);
        log("探针(深1) = " + flatProbe + "  (" + PROBE_BYTES + " 字节, 相对深度 1)");
        log("探针(深5) = " + deepProbe + "  (" + PROBE_BYTES + " 字节, 相对深度 5)");
        log("期望引擎   = " + engine);

        Linker linker = Linker.nativeLinker();
        // 清理必须放在 finally：断言失败时异常会直接抛出探针目录树，
        // 对着长期存在的扫描根反复跑就会不断堆积垃圾。
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

                // 以下两条与引擎无关：每个平台都必须真的检索到东西并带回正确 size。
                // walkdir 分支若只断言 count==0，「永远返回空」「size 恒为 0」的构建会全绿。
                require(count >= 1, "连深度 1 的探针都搜不到，引擎没有返回任何结果: " + json);
                require(json.contains("\"size\":" + PROBE_BYTES),
                        "未回传正确大小（期望 " + PROBE_BYTES + "）: " + json);

                // 引擎判别：同名探针，深度 1 两条引擎都命中，深度 5 只有 MFT 能命中
                if ("mft".equals(engine)) {
                    require(count == 2,
                            "MFT 应同时命中深度 1 与深度 5 两个同名探针，实际 " + count + "  " + json);
                } else {
                    require(count == 1,
                            "walkdir(max_depth=3) 只应命中深度 1 的探针，实际 " + count + "  " + json);
                }
            }
        } finally {
            deleteQuietly(flatProbe);
            deleteQuietly(deepRoot);
        }

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
     * 尽力删除探针。
     *
     * <p>传入<b>目录</b>时必须传本次建探针用的根目录，而不是最深处的那个文件：
     * {@code Files.walk} 传入文件路径时只产出该文件自身，不会向上展开目录树；
     * 传文件或只传文件的父目录，都会把更上层的空目录留在磁盘根上。
     * 逆序遍历保证先删子、后删父。</p>
     *
     * <p>传入<b>文件</b>时（深度 1 探针）这正是想要的语义：只删该文件本身。</p>
     *
     * @param target 本次创建的探针根目录或探针文件
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
