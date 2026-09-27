package com.chua.filesearch.support.bridge.jna;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.util.Locale;

/**
 * {@code file_search} 原生库的 classpath 抽取器（Java 8，零第三方依赖）。
 *
 * <p>本模块刻意不依赖 {@code utils-support-common-starter} 的 {@code NativeLoader}：
 * 该 starter 已按 {@code release 25} 编译，一旦引入，Java 8 运行时会在类加载阶段
 * 直接抛 {@code UnsupportedClassVersionError}，与“Java 8 可用”的目标冲突。</p>
 *
 * <p>抽取策略：按 {@code os.name} / {@code os.arch} 定位 jar 内
 * {@code /native/{os}-{arch}/{动态库文件名}}，释放到 {@code java.io.tmpdir} 下的
 * 独立子目录后交给 JNA 加载。若 classpath 中缺产物，可用系统属性
 * {@code chua.filesearch.native.path} 直接指定动态库绝对路径。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
final class NativeLibraryExtractor {

    /**
     * 原生库逻辑名（不含 lib 前缀与后缀）
     */
    private static final String LIBRARY_NAME = "file_search";

    /**
     * jar 内原生库资源根目录
     */
    private static final String RESOURCE_ROOT = "/native/";

    /**
     * 抽取到临时目录时使用的子目录名
     */
    private static final String TEMP_SUB_DIR = "chua-native-file-search-java8";

    /**
     * 直接指定动态库路径的系统属性名
     */
    private static final String OVERRIDE_PROPERTY = "chua.filesearch.native.path";

    /**
     * 工具类，禁止实例化。
     */
    private NativeLibraryExtractor() {
    }

    /**
     * 定位并释放当前平台的原生库。
     *
     * <p>优先使用系统属性覆盖，其次从 classpath 抽取。</p>
     *
     * @return 动态库在本地的绝对路径
     * @throws IOException 资源缺失或抽取失败
     */
    static Path resolve() throws IOException {
        String override = System.getProperty(OVERRIDE_PROPERTY);
        if (override != null && !override.trim().isEmpty()) {
            Path path = Paths.get(override.trim());
            if (!Files.isRegularFile(path)) {
                throw new IOException("系统属性 " + OVERRIDE_PROPERTY + " 指向的文件不存在: " + path);
            }
            return path.toAbsolutePath();
        }
        String platform = platformDir();
        String fileName = libraryFileName();
        String resource = RESOURCE_ROOT + platform + "/" + fileName;
        try (InputStream in = NativeLibraryExtractor.class.getResourceAsStream(resource)) {
            if (in == null) {
                throw new IOException("classpath 缺少原生库资源 " + resource
                        + "，可用 -D" + OVERRIDE_PROPERTY + "=<动态库绝对路径> 手动指定");
            }
            Path tempDir = Paths.get(System.getProperty("java.io.tmpdir"), TEMP_SUB_DIR);
            Files.createDirectories(tempDir);
            Path target = Files.createTempFile(tempDir, LIBRARY_NAME + "_", "_" + fileName);
            Files.copy(in, target, StandardCopyOption.REPLACE_EXISTING);
            target.toFile().deleteOnExit();
            return target.toAbsolutePath();
        }
    }

    /**
     * 计算当前平台目录名（如 {@code windows-x86_64}）。
     *
     * @return 平台目录名；不支持的系统抛 {@link UnsupportedOperationException}
     */
    static String platformDir() {
        return osPrefix() + "-" + normalizeArch(System.getProperty("os.arch", "").toLowerCase(Locale.ROOT));
    }

    /**
     * 计算当前平台动态库文件名（含 lib 前缀与后缀）。
     *
     * @return 动态库文件名
     */
    static String libraryFileName() {
        String prefix = osPrefix();
        if ("windows".equals(prefix)) {
            return LIBRARY_NAME + ".dll";
        }
        if ("darwin".equals(prefix)) {
            return "lib" + LIBRARY_NAME + ".dylib";
        }
        return "lib" + LIBRARY_NAME + ".so";
    }

    /**
     * 解析操作系统前缀。
     *
     * @return {@code windows} / {@code darwin} / {@code linux}
     */
    private static String osPrefix() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        if (os.contains("win")) {
            return "windows";
        }
        if (os.contains("mac")) {
            return "darwin";
        }
        if (os.contains("linux")) {
            return "linux";
        }
        throw new UnsupportedOperationException("不支持的操作系统: " + os);
    }

    /**
     * 归一化 CPU 架构名，与产物目录命名保持一致。
     *
     * @param arch 原始架构名（已转小写）
     * @return 归一化后的架构名
     */
    private static String normalizeArch(String arch) {
        if ("amd64".equals(arch) || "x86_64".equals(arch) || "x64".equals(arch) || "em64t".equals(arch)) {
            return "x86_64";
        }
        if ("aarch64".equals(arch) || "arm64".equals(arch)) {
            return "aarch64";
        }
        return arch;
    }
}
