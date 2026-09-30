package com.chua.nativesysinformer.java8;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.util.Locale;

/**
 * {@code sysinformer} 原生库的 classpath 抽取器（Java 8，零项目依赖）。
 *
 * <p>本类刻意不依赖 {@code utils-support-common-starter} 的 {@code NativeLoader}：
 * 该 starter 已按 {@code release 25} 编译，一旦引入，JDK 8 运行时会在类加载阶段
 * 直接抛 {@code UnsupportedClassVersionError}，与「Java 8 可用」的目标冲突。</p>
 *
 * <p>抽取策略：按 {@code os.name} / {@code os.arch} 定位 jar 内
 * {@code /native/{os}-{arch}/{动态库文件名}}，释放到 {@code java.io.tmpdir} 下的
 * 独立子目录后交给 JNA 加载。若 classpath 中缺产物，可用系统属性
 * {@code chua.sysinformer.native.path} 直接指定动态库绝对路径。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
final class NativeLibraryExtractor {

    /**
     * 原生库逻辑名（不含 lib 前缀与后缀）。
     */
    private static final String LIBRARY_NAME = "sysinformer";

    /**
     * jar 内原生库资源根目录。
     */
    private static final String RESOURCE_ROOT = "/native/";

    /**
     * 抽取到临时目录时使用的子目录名。
     */
    private static final String TEMP_SUB_DIR = "chua-native-sysinformer-java8";

    /**
     * 直接指定动态库路径的系统属性名。
     */
    private static final String OVERRIDE_PROPERTY = "chua.sysinformer.native.path";

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
    static Path extract() throws IOException {
        String override = System.getProperty(OVERRIDE_PROPERTY);
        if (override != null && override.trim().length() > 0) {
            Path p = Paths.get(override.trim());
            if (!Files.isRegularFile(p)) {
                throw new IOException("系统属性 " + OVERRIDE_PROPERTY + " 指向的文件不存在: " + p);
            }
            return p;
        }

        String platform = platformDir();
        String fileName = libraryFileName();
        String resource = RESOURCE_ROOT + platform + "/" + fileName;

        InputStream in = NativeLibraryExtractor.class.getResourceAsStream(resource);
        if (in == null) {
            throw new IOException("classpath 中找不到原生库 " + resource
                    + "；可设置 -D" + OVERRIDE_PROPERTY + "=<动态库绝对路径> 指定");
        }

        Path targetDir = Paths.get(System.getProperty("java.io.tmpdir"), TEMP_SUB_DIR, platform);
        Files.createDirectories(targetDir);
        Path target = targetDir.resolve(fileName);
        try {
            Files.copy(in, target, StandardCopyOption.REPLACE_EXISTING);
        } finally {
            closeQuietly(in);
        }
        return target;
    }

    /**
     * 推导当前平台的资源目录名，形如 {@code windows-x86_64}。
     *
     * @return 平台目录名
     * @throws IOException 平台不受支持
     */
    static String platformDir() throws IOException {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);

        String osName;
        if (os.contains("win")) {
            osName = "windows";
        } else if (os.contains("mac") || os.contains("darwin")) {
            osName = "darwin";
        } else if (os.contains("linux")) {
            osName = "linux";
        } else {
            throw new IOException("不支持的操作系统: " + os);
        }

        // os.arch 在不同 JVM 上有多种写法：amd64/x86_64、aarch64/arm64。
        // 消费目录统一为 x86_64 与 aarch64，必须归一化，否则会去找不存在的目录。
        String archName;
        if (arch.contains("aarch64") || arch.contains("arm64")) {
            archName = "aarch64";
        } else if (arch.contains("64") || arch.contains("amd64") || arch.contains("x86_64")) {
            archName = "x86_64";
        } else {
            throw new IOException("不支持的处理器架构: " + arch);
        }
        return osName + "-" + archName;
    }

    /**
     * 推导当前平台的动态库文件名。
     *
     * @return 文件名，如 {@code sysinformer.dll} / {@code libsysinformer.so}
     * @throws IOException 平台不受支持
     */
    static String libraryFileName() throws IOException {
        String platform = platformDir();
        if (platform.startsWith("windows")) {
            return LIBRARY_NAME + ".dll";
        }
        if (platform.startsWith("darwin")) {
            return "lib" + LIBRARY_NAME + ".dylib";
        }
        return "lib" + LIBRARY_NAME + ".so";
    }

    /**
     * 静默关闭流。
     *
     * @param in 流，可为 null
     */
    private static void closeQuietly(InputStream in) {
        if (in == null) {
            return;
        }
        try {
            in.close();
        } catch (IOException ignored) {
            // 关闭失败不影响抽取结果
        }
    }

    /**
     * 抽取目录是否已存在（供诊断）。
     *
     * @return 抽取目录路径
     */
    static File tempDir() {
        return new File(System.getProperty("java.io.tmpdir"), TEMP_SUB_DIR);
    }
}