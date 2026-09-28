import com.chua.filesearch.support.bridge.jna.FileSearchResult;
import com.chua.filesearch.support.bridge.jna.JnaFileSearchBridge;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;

/**
 * Java 8 + JNA 侧的「NTFS MFT 直读 vs walkdir 回退」引擎断言冒烟。
 *
 * <p>本模块是 JDK 8 环境下 {@code file_search} 唯一正式的绑定，但它原有的
 * {@link FilesearchJnaBridgeSmoke} 只在临时目录上验证功能，
 * <b>无法判定本次运行到底走了 MFT 直读还是 walkdir 回退</b>：两条路径都能搜到临时目录里的文件。
 * 本冒烟补上这个判定能力，断言逻辑与 Java 25 侧
 * {@code utils-support-native-filesearch} 模块的 {@code FilesearchMftSmoke} 逐条对齐。</p>
 *
 * <p>原理：在扫描根目录下放置一个<b>相对深度 5</b> 的探针文件。walkdir 的
 * {@code WalkDir::max_depth(3)} 永远看不到它，只有 MFT 直读能命中。因此同一个探针在两种引擎下
 * 结果必然不同，可据此断言实际路径，并顺带校验未命名 {@code $DATA} 的 size 是否被正确回传。</p>
 *
 * <p>用法（需 JDK 8）：</p>
 * <pre>
 *   mvn -q -B -f utils-support-native-filesearch-java8/pom.xml -DskipTests package
 *   mvn -q -B -f utils-support-native-filesearch-java8/pom.xml \
 *     dependency:build-classpath "-Dmdep.outputFile=target/cp.txt"
 *   javac -encoding UTF-8 -nowarn -cp "utils-support-native-filesearch-java8/target/classes:$(cat utils-support-native-filesearch-java8/target/cp.txt)" \
 *     -d target/smoke src/smoke/java/FilesearchMftJnaSmoke.java
 *   java "-Dchua.filesearch.native.path=&lt;动态库绝对路径&gt;" \
 *     -cp "target/smoke:utils-support-native-filesearch-java8/target/classes:$(cat utils-support-native-filesearch-java8/target/cp.txt)" \
 *     FilesearchMftJnaSmoke &lt;扫描根目录&gt; mft|walkdir
 * </pre>
 *
 * <p>预期：Windows + 管理员 → {@code mft}（命中且带回正确 size）；
 * 其它平台或非管理员 → {@code walkdir}（不命中）。</p>
 *
 * <p>与 Java 25 侧 FFM 冒烟共用同一个原生符号
 * {@code Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName}，
 * 即两条 Java 通路跑的是同一份 Rust MFT 代码，本冒烟可作为 FFM 侧结果的交叉印证。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class FilesearchMftJnaSmoke {

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
    private FilesearchMftJnaSmoke() {
    }

    /**
     * 冒烟入口。
     *
     * @param argv [0]=扫描根目录 [1]=期望引擎（mft|walkdir）
     * @throws Exception 任何失败
     */
    public static void main(String[] argv) throws Exception {
        if (argv.length < 2) {
            System.err.println("用法: FilesearchMftJnaSmoke <扫描根目录> <mft|walkdir>");
            System.exit(2);
        }
        Path root = Paths.get(argv[0]);
        String engine = argv[1];

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

        JnaFileSearchBridge.loadLibrary();
        if (!JnaFileSearchBridge.isLoaded()) {
            throw new AssertionError("ASSERT FAIL 原生库加载失败: " + JnaFileSearchBridge.getLoadError());
        }
        log("动态库路径 = " + JnaFileSearchBridge.getLibraryPath());
        log("版本串     = " + JnaFileSearchBridge.getVersion());

        final List<FileSearchResult> hits = new ArrayList<FileSearchResult>();
        long startNanos = System.nanoTime();
        int count = JnaFileSearchBridge.searchByName(root.toString(), tag + ".txt", 10, hits::add);
        long costMillis = (System.nanoTime() - startNanos) / 1000000L;
        log("原生调用耗时 = " + costMillis + " ms");
        log("返回 count   = " + count + "  (回调收到 " + hits.size() + " 条)");

        if ("mft".equals(engine)) {
            require(count >= 1, "MFT 直读命中深度 5 探针 -> count = " + count);
            require(!hits.isEmpty(), "回调收到结果 -> " + hits.size() + " 条");
            long actualSize = hits.get(0).getSize();
            require(actualSize == PROBE_BYTES,
                    "$DATA 大小回传正确 -> " + actualSize + " 字节");
            String actualPath = hits.get(0).getPath();
            require(actualPath.contains(tag + ".txt"), "结果路径含探针文件名 -> " + actualPath);
            log("命中条目   = " + actualPath
                    + "  ext=" + hits.get(0).getExtension()
                    + "  modified=" + hits.get(0).getLastModified());
        } else {
            require(count == 0,
                    "walkdir(max_depth=3) 不命中深度 5 探针 -> count = " + count);
        }

        JnaFileSearchBridge.cancel();
        deleteQuietly(probeRoot);
        log("SMOKE OK");
        System.out.println(LOG);
    }

    /**
     * 断言。
     *
     * @param condition 条件
     * @param message   断言通过时要打印的结论（含实测值）
     * @return 恒为 true
     * @throws AssertionError 条件不成立
     */
    private static boolean require(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError("ASSERT FAIL " + message);
        }
        log("ASSERT ok   " + message);
        return true;
    }

    /**
     * 尽力删除探针目录树。
     *
     * <p>入参必须是本次创建探针时用的<b>根目录</b>，而不是探针文件：
     * {@code Files.walk} 传入文件路径时只产出该文件自身，不会向上展开目录树；
     * 传文件或只传文件的父目录，都会把更上层的空目录留在扫描根上。
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
