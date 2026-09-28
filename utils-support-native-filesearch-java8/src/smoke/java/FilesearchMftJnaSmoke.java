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
 * <p>原理：在扫描根目录下放置两个<b>同名</b>探针文件，只有目录深度不同。深度 1 的两条引擎都
 * 必须命中；深度 5 的 walkdir {@code WalkDir::max_depth(3)} 永远看不到，只有 MFT 直读能命中。
 * 于是同一次搜索在 MFT 下应返回 <b>2</b> 条、在 walkdir 下应返回 <b>1</b> 条，差值即本次实际
 * 走的是哪条路径，并顺带校验未命名 {@code $DATA} 的 size 是否被正确回传。</p>
 *
 * <p>深度 1 探针是与引擎无关的<b>正向断言</b>：少了它，walkdir 分支只剩 {@code count == 0}
 * 一条断言，一个「永远返回空」「size 恒为 0」的构建照样全绿。</p>
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
 * <p>预期：Windows + 管理员 → {@code mft}（count=2）；
 * 其它平台或非管理员 → {@code walkdir}（count=1）。</p>
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
        // 两个探针同名，只差目录深度：同一次搜索即可同时验「能不能搜到」和「走的是哪条路」。
        // 深度 1 探针是与引擎无关的正向断言——少了它，walkdir 分支只剩 count == 0，
        // 一个「永远返回空」「size 恒为 0」的构建照样全绿。
        Path flatProbe = root.resolve(tag + ".txt");
        Path probeRoot = root.resolve(tag + "-1");
        Path deepProbe = probeRoot.resolve(tag + "-2").resolve(tag + "-3")
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

        JnaFileSearchBridge.loadLibrary();
        if (!JnaFileSearchBridge.isLoaded()) {
            throw new AssertionError("ASSERT FAIL 原生库加载失败: " + JnaFileSearchBridge.getLoadError());
        }
        log("动态库路径 = " + JnaFileSearchBridge.getLibraryPath());
        log("版本串     = " + JnaFileSearchBridge.getVersion());

        final List<FileSearchResult> hits = new ArrayList<FileSearchResult>();
        long startNanos = System.nanoTime();
        int count;
        try {
            count = JnaFileSearchBridge.searchByName(root.toString(), tag + ".txt", 10, hits::add);
            long costMillis = (System.nanoTime() - startNanos) / 1000000L;
            log("原生调用耗时 = " + costMillis + " ms");
            log("返回 count   = " + count + "  (回调收到 " + hits.size() + " 条)");

            // 以下两条与引擎无关：每个平台都必须真的检索到东西并带回正确 size。
            // walkdir 分支若只断言 count == 0，「永远返回空」「size 恒为 0」的构建会全绿。
            require(count >= 1, "连深度 1 的探针都搜不到，JNA 回调没有返回任何结果 -> count = " + count);
            require(!hits.isEmpty(), "回调收到结果 -> " + hits.size() + " 条");
            require(hits.get(0).getSize() == PROBE_BYTES,
                    "$DATA 大小回传正确 -> " + hits.get(0).getSize() + " 字节");

            // 引擎判别：同名探针，深度 1 两条引擎都命中，深度 5 只有 MFT 能命中
            if ("mft".equals(engine)) {
                require(count == 2,
                        "MFT 应同时命中深度 1 与深度 5 两个同名探针 -> count = " + count);
            } else {
                require(count == 1,
                        "walkdir(max_depth=3) 只应命中深度 1 的探针 -> count = " + count);
            }
            for (FileSearchResult hit : hits) {
                log("命中条目   = " + hit.getPath()
                        + "  size=" + hit.getSize()
                        + "  ext=" + hit.getExtension()
                        + "  modified=" + hit.getLastModified());
            }
        } finally {
            // 清理必须放在 finally：断言失败时异常直接抛出，探针目录树会留在磁盘上
            JnaFileSearchBridge.cancel();
            deleteQuietly(flatProbe);
            deleteQuietly(probeRoot);
        }
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
