import com.chua.filesearch.support.bridge.jna.FileSearchResult;
import com.chua.filesearch.support.bridge.jna.JnaFileSearchBridge;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/**
 * Java 8 + JNA 文件搜索冒烟测试：只依赖本模块产物与 jna.jar，不依赖任何 release 25 组件。
 *
 * <p>用法：</p>
 * <pre>
 *   javac -encoding UTF-8 -cp "target/classes;%USERPROFILE%/.m2/repository/net/java/dev/jna/jna/5.14.0/jna-5.14.0.jar" \
 *     -d target/smoke src/smoke/java/FilesearchJnaBridgeSmoke.java
 *   java -cp "target/classes;target/smoke;jna-5.14.0.jar;slf4j-api-1.7.36.jar" FilesearchJnaBridgeSmoke
 * </pre>
 *
 * <p>原生库默认从 classpath {@code /native/{os}-{arch}/} 抽取；也可用
 * {@code -Dchua.filesearch.native.path=<动态库绝对路径>} 覆盖。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class FilesearchJnaBridgeSmoke {

    /**
     * 执行日志缓冲区
     */
    private static final StringBuilder LOG = new StringBuilder();

    /**
     * 工具类，禁止实例化。
     */
    private FilesearchJnaBridgeSmoke() {
    }

    /**
     * 冒烟入口。
     *
     * @param args 忽略
     * @throws Exception 任何失败
     */
    public static void main(String[] args) throws Exception {
        Path tmp = Files.createTempDirectory("fs-jna-smoke");
        for (int i = 1; i <= 3; i++) {
            Files.write(tmp.resolve("s" + i + ".log"), "x".getBytes(StandardCharsets.UTF_8));
        }
        Files.write(tmp.resolve("other.txt"), "y".getBytes(StandardCharsets.UTF_8));
        log("临时目录 = " + tmp);

        JnaFileSearchBridge.loadLibrary();
        if (!JnaFileSearchBridge.isLoaded()) {
            throw new AssertionError("ASSERT FAIL 原生库加载失败: " + JnaFileSearchBridge.getLoadError());
        }
        log("ASSERT ok   原生库已加载");
        log("动态库路径 = " + JnaFileSearchBridge.getLibraryPath());

        String version = JnaFileSearchBridge.getVersion();
        require(version != null && !version.isEmpty(), "版本串为空");
        log("fast_get_version = " + version);

        List<FileSearchResult> byName = new ArrayList<>();
        int nameCount = JnaFileSearchBridge.searchByName(tmp.toString(), "*.log", 100, byName::add);
        log("searchByName(*.log) count = " + nameCount);
        require(nameCount == 3, "预期命中 3 个 .log，实际 " + nameCount);
        require(byName.size() == 3, "回调条目数应为 3，实际 " + byName.size());
        require(byName.get(0).getPath().contains("s"), "结果路径异常: " + byName.get(0).getPath());

        List<FileSearchResult> tree = new ArrayList<>();
        int treeCount = JnaFileSearchBridge.getTree(tmp.toString(), 3, 100, tree::add);
        log("getTree count = " + treeCount);
        require(treeCount >= 4, "遍历至少应命中 4 个文件，实际 " + treeCount);

        List<FileSearchResult> bySize = new ArrayList<>();
        int sizeCount = JnaFileSearchBridge.searchBySize(tmp.toString(), 0L, Long.MAX_VALUE, 100, bySize::add);
        log("searchBySize(0..MAX) count = " + sizeCount);
        require(sizeCount >= 4, "按大小过滤至少应命中 4 个文件，实际 " + sizeCount);

        List<FileSearchResult> byPath = new ArrayList<>();
        int pathCount = JnaFileSearchBridge.searchByPath(tmp.toString(), "*other.txt", 100, byPath::add);
        log("searchByPath(*other.txt) count = " + pathCount);
        require(pathCount == 1, "按路径过滤应命中 1 个文件，实际 " + pathCount);

        JnaFileSearchBridge.cancel();

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
     * 记录一行日志。
     *
     * @param line 日志内容
     */
    private static void log(String line) {
        LOG.append(line).append(System.lineSeparator());
    }
}
