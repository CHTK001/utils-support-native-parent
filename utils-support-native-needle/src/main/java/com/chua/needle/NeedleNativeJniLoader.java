package com.chua.needle;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.stream.Stream;

/**
 * Needle 推理引擎原生库加载器。
 *
 * <p><b>本仓库不包含 {@code chua_native_needle} 的二进制</b>，仅有本门面。
 * 因此加载顺序为：先尝试外部目录覆盖，再回退 classpath 抽取，两者都找不到
 * 时记录失败原因（不抛异常），由 {@link NeedleNative} 统一对外暴露状态。</p>
 *
 * <p>外部目录覆盖适用于不方便把动态库打进 jar 的场景，按以下优先级取值：</p>
 * <ol>
 *   <li>系统属性 {@code -Dchua.needle.native.dir=<目录>}</li>
 *   <li>环境变量 {@code CHUA_NEEDLE_NATIVE_DIR=<目录>}</li>
 * </ol>
 * <p>该目录下匹配 {@code *needle*} 且以 {@code .dll/.so/.dylib} 结尾的文件
 * 会被直接 {@link System#load(Path)} 加载。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
final class NeedleNativeJniLoader {

    /**
     * 外部目录覆盖所用的系统属性名
     */
    private static final String PROP_NATIVE_DIR = "chua.needle.native.dir";

    /**
     * 外部目录覆盖所用的环境变量名
     */
    private static final String ENV_NATIVE_DIR = "CHUA_NEEDLE_NATIVE_DIR";

    /**
     * 原生库逻辑名
     */
    private static final String LIBRARY_NAME = "chua_native_needle";

    /**
     * 原生库文件名匹配模式
     */
    private static final String LIBRARY_GLOB = "*needle*";

    /**
     * 是否已加载成功
     */
    private static volatile boolean loaded;

    /**
     * 首次加载失败的原因；加载成功时为 null
     */
    private static volatile Throwable loadError;

    /**
     * 是否已经尝试过加载（无论成败），用于保证只尝试一次
     */
    private static volatile boolean attempted;

    private NeedleNativeJniLoader() {
    }

    /**
     * 原生库是否加载成功。
     *
     * @return 加载成功返回 true
     */
    static boolean isLoaded() {
        return loaded;
    }

    /**
     * 获取首次加载失败的原因。
     *
     * @return 失败异常；未尝试过或加载成功时返回 null
     */
    static Throwable loadError() {
        return loadError;
    }

    /**
     * 执行加载，只尝试一次。
     *
     * <p>本方法不抛异常：失败原因记录到 {@link #loadError()}，避免类初始化阶段
     * 因缺少可选原生库而整体失败。实际调用推理接口时再由
     * {@link NeedleNative} 抛出带原因的异常。</p>
     */
    static void load() {
        if (attempted) {
            return;
        }
        synchronized (NeedleNativeJniLoader.class) {
            if (attempted) {
                return;
            }
            attempted = true;
            try {
                loadFromExternalDir();
            } catch (Throwable external) {
                try {
                    loadFromClasspath();
                } catch (Throwable fromClasspath) {
                    loadError = new IllegalStateException(
                            "未找到 " + LIBRARY_NAME + " 原生库（已尝试外部目录与 classpath）。"
                                    + "该库不随本仓库分发，需自行提供后通过 -D" + PROP_NATIVE_DIR
                                    + "=<目录> 或环境变量 " + ENV_NATIVE_DIR + " 指定。",
                            fromClasspath);
                    loadError.addSuppressed(external);
                }
            }
        }
    }

    /**
     * 从外部目录覆盖路径加载。
     *
     * @throws IOException  目录不可读时抛出
     * @throws UnsatisfiedLinkError 目录下没有匹配文件时抛出
     */
    private static void loadFromExternalDir() throws IOException {
        String configured = System.getProperty(PROP_NATIVE_DIR);
        if (configured == null || configured.isBlank()) {
            configured = System.getenv(ENV_NATIVE_DIR);
        }
        if (configured == null || configured.isBlank()) {
            throw new IllegalStateException("未配置外部原生库目录");
        }
        Path dir = Path.of(configured);
        if (!Files.isDirectory(dir)) {
            throw new IOException("外部原生库目录不存在：" + dir.toAbsolutePath());
        }
        Path library;
        try (Stream<Path> entries = Files.list(dir)) {
            List<Path> matched = entries
                    .filter(Files::isRegularFile)
                    .filter(path -> matchesLibraryName(path.getFileName().toString()))
                    .toList();
            if (matched.isEmpty()) {
                throw new UnsatisfiedLinkError(
                        "外部目录内没有匹配 " + LIBRARY_GLOB + " 的动态库：" + dir.toAbsolutePath());
            }
            library = matched.get(0);
        }
        System.load(library.toAbsolutePath().toString());
    }

    /**
     * 从 classpath 的 native 目录抽取并加载。
     */
    private static void loadFromClasspath() {
        Path target = NativeUtils.tempRoot().resolve(LIBRARY_NAME);
        NativeLoader.of(LIBRARY_NAME)
                .glob(LIBRARY_GLOB)
                .toTarget(target)
                .load();
    }

    /**
     * 判断文件名是否像本模块的原生库。
     *
     * @param fileName 文件名
     * @return 名称含 needle 且为动态库扩展名返回 true
     */
    private static boolean matchesLibraryName(String fileName) {
        String lower = fileName.toLowerCase(Locale.ROOT);
        return lower.contains("needle")
                && (lower.endsWith(".dll") || lower.endsWith(".so") || lower.endsWith(".dylib"));
    }
}
