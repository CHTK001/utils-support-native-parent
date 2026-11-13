package com.chua.needle;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;

import java.io.IOException;
import java.lang.foreign.Arena;
import java.lang.foreign.SymbolLookup;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.stream.Stream;

/**
 * Needle 推理引擎动态库的发现与加载。
 *
 * <p><b>本仓库不包含 needle 引擎二进制</b>。引擎由 cactus-compute 官方发布，
 * 命名为 {@code libneedle3.dll} / {@code libneedle3.so} / {@code libneedle3.dylib}
 * （Needle 2 为 {@code libneedle2.*}，两代是两个不同的库文件，不可混用），
 * 需自行从 HuggingFace 获取后提供。查找顺序：</p>
 * <ol>
 *   <li>外部目录：系统属性 {@code -Dchua.needle.native.dir=<目录>}</li>
 *   <li>环境变量 {@code CHUA_NEEDLE_NATIVE_DIR=<目录>}</li>
 *   <li>classpath 中 {@code native/**}{@code /*needle*}{@code .*} 抽取</li>
 * </ol>
 * <p>外部目录下匹配 {@code *needle*} 且以 {@code .dll/.so/.dylib} 结尾的文件会被加载。</p>
 *
 * <p>加载使用 {@link SymbolLookup#libraryLookup} 而非 {@link System#load}，
 * 以便同一步拿到符号查找器供 {@link NeedleEngine} 绑定 FFM 下调句柄。
 * 库以 {@link Arena#global()} 加载，进程生命周期内不卸载——引擎为全局单例且
 * 权重不可卸载，提前卸载会导致后续调用崩溃。</p>
 *
 * <p>本类不抛异常：失败原因记录到 {@link #loadError()}，避免类初始化阶段因缺少
 * 可选原生库而整体失败；实际调用推理接口时由 {@link NeedleNative} 抛出。</p>
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
     * 原生库文件名匹配模式
     */
    private static final String LIBRARY_GLOB = "*needle*";

    /**
     * 引擎库加载使用的全局 Arena，保证进程内不卸载
     */
    private static final Arena LIBRARY_ARENA = Arena.global();

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
     * 外部目录内只有 CLI 运行器、缺少共享库时的专用诊断。
     *
     * <p>单独建型而非复用 {@link IOException}，以便 {@link #load()} 在回退到
     * classpath 失败后，仍以本诊断作为主因而非被通用文案覆盖。</p>
     */
    private static final class RunnerOnlyException extends IOException {

        private static final long serialVersionUID = 1L;

        RunnerOnlyException(String message) {
            super(message);
        }
    }

    /**
     * 动态库是否加载成功且符号已解析。
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
     * <p>本方法不抛异常：失败原因记录到 {@link #loadError()}。</p>
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
            Path library = null;
            String specific = null;
            try {
                library = locateFromExternalDir();
            } catch (RunnerOnlyException e) {
                // 外部目录里只有 CLI 运行器：这是最常见的误配，保留其诊断信息
                specific = e.getMessage();
                try {
                    library = extractFromClasspath();
                } catch (Throwable fromClasspath) {
                    loadError = new IllegalStateException(specific, fromClasspath);
                    return;
                }
            } catch (Throwable external) {
                try {
                    library = extractFromClasspath();
                } catch (Throwable fromClasspath) {
                    loadError = new IllegalStateException(
                            "未找到 needle 引擎共享库（已尝试外部目录与 classpath）。"
                                    + "该库由 cactus-compute 官方发布、不随本仓库分发。"
                                    + "获取方式：pip install cactus-needle 后取 "
                                    + "~/.cache/cactus-needle/v3/<版本>/libneedle3.dll，"
                                    + "或从 HuggingFace 的 python/cactus_needle-*-py3-none-<平台标签>.whl "
                                    + "中抽取 needle/libneedle3.dll；再通过 -D" + PROP_NATIVE_DIR
                                    + "=<目录> 或环境变量 " + ENV_NATIVE_DIR + " 指定该目录。",
                            fromClasspath);
                    loadError.addSuppressed(external);
                    return;
                }
            }
            try {
                SymbolLookup lookup = SymbolLookup.libraryLookup(
                        library.toAbsolutePath().toString(), LIBRARY_ARENA);
                NeedleEngine.bind(lookup);
                NeedleEngine.libraryLocated(library);
                loaded = true;
            } catch (Throwable t) {
                loadError = new IllegalStateException(
                        "加载 needle 引擎失败：" + library.toAbsolutePath() + "；" + t.getMessage(), t);
            }
        }
    }

    /**
     * 从外部目录覆盖路径定位引擎库。
     *
     * @return 引擎库路径
     * @throws IOException 目录不可读或没有匹配文件时抛出
     */
    private static Path locateFromExternalDir() throws IOException {
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
        List<Path> entries;
        try (Stream<Path> stream = Files.list(dir)) {
            entries = stream.filter(Files::isRegularFile).toList();
        }
        List<Path> matched = entries.stream()
                .filter(path -> matchesLibraryName(path.getFileName().toString()))
                .toList();
        if (matched.isEmpty()) {
            // 官方在 <platform>/ 目录下分发的是 CLI 运行器（needle / needle.exe），
            // 它们没有导出表，不是可加载的共享库。命中时给出准确原因，避免用户
            // 反复在平台目录里找 libneedle3。
            boolean onlyRunner = entries.stream()
                    .map(path -> path.getFileName().toString())
                    .anyMatch(NeedleNativeJniLoader::isRunnerName);
            if (onlyRunner) {
                throw new RunnerOnlyException("外部目录内只有 needle CLI 运行器"
                        + "（needle / needle.exe），它不导出 C API，无法被 FFM 加载："
                        + dir.toAbsolutePath() + "。共享库请另行获取——最简单是 "
                        + "pip install cactus-needle 后取 "
                        + "~/.cache/cactus-needle/v3/<版本>/libneedle3.dll，"
                        + "或从 HuggingFace 的 python/cactus_needle-*-py3-none-<平台标签>.whl "
                        + "中抽取 needle/libneedle3.dll。");
            }
            throw new IOException("外部目录内没有匹配 " + LIBRARY_GLOB
                    + " 的动态库：" + dir.toAbsolutePath());
        }
        // 优先取 Needle 3，避免误加载同目录下的 Needle 2
        return matched.stream()
                .filter(path -> path.getFileName().toString().contains("3"))
                .findFirst()
                .orElse(matched.get(0));
    }

    /**
     * 判断是否为官方分发的 CLI 运行器（无导出表，不可加载）。
     *
     * @param fileName 文件名
     * @return 是运行器返回 true
     */
    private static boolean isRunnerName(String fileName) {
        return fileName.equals("needle") || fileName.equals("needle.exe");
    }

    /**
     * 从 classpath 抽取引擎库。
     *
     * <p>使用 {@code extractOnly(true)} 只抽取不 {@code System.load}：真正的加载
     * 交由 {@link SymbolLookup#libraryLookup} 完成，避免同一份库被两条路径重复加载。
     * 抽取后的文件名保持 classpath 中的原名，故此处按 glob 重新定位。</p>
     *
     * @return 抽取后的引擎库路径
     * @throws IOException 抽取后目录内没有匹配文件时抛出
     */
    private static Path extractFromClasspath() throws IOException {
        Path targetDir = NativeUtils.tempRoot().resolve("libneedle3");
        NativeLoader.of("libneedle3")
                .glob(LIBRARY_GLOB)
                .toTarget(targetDir)
                .extractOnly(true)
                .load();
        try (Stream<Path> entries = Files.list(targetDir)) {
            return entries
                    .filter(Files::isRegularFile)
                    .filter(path -> matchesLibraryName(path.getFileName().toString()))
                    .max(Comparator.comparing(path ->
                            path.getFileName().toString().contains("3")))
                    .orElseThrow(() -> new IOException(
                            "抽取目录内没有匹配 " + LIBRARY_GLOB + " 的动态库："
                                    + targetDir.toAbsolutePath()));
        }
    }

    /**
     * 判断文件名是否像 needle 引擎库。
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
