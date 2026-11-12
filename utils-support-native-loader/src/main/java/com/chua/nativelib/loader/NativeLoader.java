package com.chua.nativelib.loader;

import java.io.InputStream;
import java.net.JarURLConnection;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Enumeration;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;

/**
 * 支持链式、任务标识、MD5校验、固定目录的原生库加载器。
 *
 * <p>默认从 {@code native/{platformdir}/} 抽取原生库到目标目录，
 * 抽取后调用 {@link NativeUtils#loadFromClasspath} 的落点加载；
 * 可通过 {@link #extractOnly(boolean)} 仅抽取，交由 SDK 自行加载。</p>
 *
 * <p>本类只依赖 JDK，供 native 模块独立使用，不引入 common-starter。</p>
 *
 * @author CH
 * @since 4.0.0.43
 */
public final class NativeLoader {

    /**
     * 已加载任务标识
     */
    private static final ConcurrentHashMap<String, Boolean> LOADED = new ConcurrentHashMap<>();

    private final String taskId;
    private ClassLoader classLoader;
    private Path targetDir;
    private String glob;
    private boolean md5;
    private boolean extractOnly;
    private boolean useCache;
    private String basePath;

    private NativeLoader(String taskId) {
        this.taskId = taskId;
        this.classLoader = Thread.currentThread().getContextClassLoader();
        this.glob = "*.dll";
        this.md5 = true;
        this.extractOnly = false;
        this.useCache = true;
    }

    /**
     * 创建加载器。
     *
     * @param taskId 任务标识
     * @return 加载器
     */
    public static NativeLoader of(String taskId) {
        return new NativeLoader(taskId);
    }

    /**
     * 设置加载源 classloader，默认当前线程上下文 classloader。
     *
     * @param classLoader 类加载器
     * @return this
     */
    public NativeLoader from(ClassLoader classLoader) {
        this.classLoader = classLoader != null ? classLoader : this.classLoader;
        return this;
    }

    /**
     * 设置目标目录（必填）。所有匹配的原生库将拷贝到该目录。
     *
     * @param targetDir 目标目录
     * @return this
     */
    public NativeLoader toTarget(String targetDir) {
        this.targetDir = Path.of(targetDir);
        return this;
    }

    /**
     * 设置目标目录（必填）。
     *
     * @param targetDir 目标目录
     * @return this
     */
    public NativeLoader toTarget(Path targetDir) {
        this.targetDir = targetDir;
        return this;
    }

    /**
     * 设置加载通配符，如 libarcsoft_face*.dll，默认 *.dll。
     *
     * @param glob glob
     * @return this
     */
    public NativeLoader glob(String glob) {
        this.glob = glob;
        return this;
    }

    /**
     * 是否启用 MD5 校验（默认 true）。启用时只有内容变化才重新拷贝。
     *
     * @param md5 是否校验
     * @return this
     */
    public NativeLoader withMd5(boolean md5) {
        this.md5 = md5;
        return this;
    }

    /**
     * 仅抽取到目标目录，不调用 System.load。
     *
     * @param extractOnly 是否仅抽取
     * @return this
     */
    public NativeLoader extractOnly(boolean extractOnly) {
        this.extractOnly = extractOnly;
        return this;
    }

    /**
     * 是否启用全局任务标识缓存。
     *
     * @param useCache {@code true} 启用缓存（默认）；{@code false} 每次执行都重新提取
     * @return this
     */
    public NativeLoader cacheable(boolean useCache) {
        this.useCache = useCache;
        return this;
    }

    /**
     * 设置自定义 classpath 基础路径，覆盖默认的 native/{platformdir}/。
     *
     * @param basePath classpath 路径
     * @return this
     */
    public NativeLoader basePath(String basePath) {
        this.basePath = basePath;
        return this;
    }

    /**
     * 执行：按 glob 匹配 classpath 资源，拷贝到目标目录；
     * 非 extractOnly 时再按文件名排序 System.load。
     */
    public void load() {
        if (useCache && LOADED.containsKey(taskId)) {
            return;
        }
        synchronized (NativeLoader.class) {
            if (useCache && LOADED.containsKey(taskId)) {
                return;
            }
            doLoad();
            if (useCache) {
                LOADED.put(taskId, Boolean.TRUE);
            }
        }
    }

    private void doLoad() {
        if (targetDir == null) {
            throw new IllegalStateException("targetDir 未设置，请先调用 toTarget()");
        }
        try {
            Files.createDirectories(targetDir);
            String resourceBase = basePath != null
                    ? basePath
                    : ("native/" + NativeUtils.getPlatformDir() + "/");
            List<ResourceItem> matched = listClasspathResources(resourceBase);
            if (matched.isEmpty()) {
                throw new UnsatisfiedLinkError("未在 classpath 找到匹配 " + glob + " 的文件: " + resourceBase);
            }

            String md5Hex = computeResourcesMd5(matched);
            Path marker = targetDir.resolve(".md5_" + md5Hex);
            boolean skipCopy = md5 && Files.exists(marker);
            if (!skipCopy) {
                for (ResourceItem item : matched) {
                    Path dest = targetDir.resolve(item.getName());
                    if (Files.notExists(dest) || Files.size(dest) != item.getSize()) {
                        try (InputStream in = item.open()) {
                            Files.copy(in, dest, StandardCopyOption.REPLACE_EXISTING);
                        }
                    }
                }
                Files.writeString(marker, md5Hex);
            }

            if (!extractOnly) {
                java.io.File[] toLoad = targetDir.toFile().listFiles((d, name) -> match(name, glob));
                if (toLoad != null && toLoad.length > 0) {
                    Arrays.sort(toLoad, (a, b) -> a.getName().compareTo(b.getName()));
                    for (java.io.File f : toLoad) {
                        NativeUtils.loadFromPathInternal(null, f.getAbsolutePath());
                    }
                }
            }
        } catch (Exception e) {
            throw new RuntimeException("NativeLoader 加载失败: taskId=" + taskId, e);
        }
    }

    private List<ResourceItem> listClasspathResources(String resourceBase) throws Exception {
        List<ResourceItem> result = new ArrayList<>();
        if (!isNativeImageRuntime()) {
            Enumeration<URL> urls = classLoader.getResources(resourceBase);
            while (urls.hasMoreElements()) {
                URL url = urls.nextElement();
                String protocol = url.getProtocol();
                if ("file".equals(protocol)) {
                    java.io.File dirFile = new java.io.File(url.toURI());
                    java.io.File[] files = dirFile.listFiles((d, name) -> match(name, glob));
                    if (files != null) {
                        for (java.io.File f : files) {
                            result.add(new ResourceItem(f.getName(), f.length(),
                                    () -> Files.newInputStream(f.toPath())));
                        }
                    }
                } else if ("jar".equals(protocol)) {
                    JarURLConnection conn = (JarURLConnection) url.openConnection();
                    try (JarFile jarFile = conn.getJarFile()) {
                        Enumeration<JarEntry> entries = jarFile.entries();
                        while (entries.hasMoreElements()) {
                            JarEntry entry = entries.nextElement();
                            String name = entry.getName();
                            if (entry.isDirectory() || !name.startsWith(resourceBase)) {
                                continue;
                            }
                            String simpleName = name.substring(resourceBase.length());
                            if (simpleName.contains("/")) {
                                continue;
                            }
                            if (!match(simpleName, glob)) {
                                continue;
                            }
                            long size = entry.getSize();
                            result.add(new ResourceItem(simpleName, size < 0 ? 0 : size,
                                    () -> classLoader.getResourceAsStream(name)));
                        }
                    }
                }
            }
        }
        if (result.isEmpty()) {
            probeClasspathResources(resourceBase, result);
        }
        return result;
    }

    private static boolean isNativeImageRuntime() {
        try {
            String code = System.getProperty("org.graalvm.nativeimage.imagecode");
            return "runtime".equals(code) || "buildtime".equals(code);
        } catch (Throwable ignored) {
            return false;
        }
    }

    private void probeClasspathResources(String resourceBase, List<ResourceItem> result) {
        List<String> basePaths = new ArrayList<>();
        if (resourceBase != null && !resourceBase.isEmpty()) {
            basePaths.add(resourceBase);
        }
        for (String dir : NativeUtils.getPlatformDirCandidates()) {
            basePaths.add("native/" + dir + "/");
        }
        basePaths.add("native/");

        for (String name : probeCandidateNames()) {
            for (String base : basePaths) {
                String full = base + name;
                try (InputStream in = classLoader.getResourceAsStream(full)) {
                    if (in != null) {
                        result.add(new ResourceItem(name, -1, () -> classLoader.getResourceAsStream(full)));
                        return;
                    }
                } catch (Exception ignored) {
                    // 继续探测下一个候选
                }
            }
        }
    }

    private List<String> probeCandidateNames() {
        Set<String> bases = new LinkedHashSet<>();
        if (!glob.contains("*") && !glob.contains("?")) {
            bases.add(glob);
        }
        if (taskId != null && !taskId.isEmpty()) {
            bases.add(taskId);
            bases.add(taskId.replace('-', '_'));
            bases.add(taskId.replace('_', '-'));
        }
        int star = glob.indexOf('*');
        int q = glob.indexOf('?');
        int cut = star < 0 ? (q < 0 ? -1 : q) : (q < 0 ? star : Math.min(star, q));
        if (cut > 0) {
            String prefix = glob.substring(0, cut);
            while (!prefix.isEmpty() && (prefix.endsWith(".") || prefix.endsWith("*") || prefix.endsWith("?"))) {
                prefix = prefix.substring(0, prefix.length() - 1);
            }
            if (!prefix.isEmpty()) {
                bases.add(prefix);
            }
        }
        List<String> names = new ArrayList<>();
        for (String b : bases) {
            names.add(NativeUtils.getLibraryFileName(b));
            names.add(NativeUtils.getLibraryFileName(b, false));
        }
        return names;
    }

    private static boolean match(String name, String glob) {
        if (name == null) {
            return false;
        }
        String regex = glob.replace(".", "\\.")
                .replace("*", ".*")
                .replace("?", ".");
        return name.matches(regex);
    }

    private static String computeResourcesMd5(List<ResourceItem> items) throws Exception {
        MessageDigest md = MessageDigest.getInstance("MD5");
        List<ResourceItem> sorted = new ArrayList<>(items);
        sorted.sort((a, b) -> a.getName().compareTo(b.getName()));
        for (ResourceItem item : sorted) {
            md.update(item.getName().getBytes());
            md.update(String.valueOf(item.getSize()).getBytes());
        }
        return bytesToHex(md.digest());
    }

    private static String bytesToHex(byte[] bytes) {
        StringBuilder sb = new StringBuilder();
        for (byte b : bytes) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }

    @FunctionalInterface
    private interface StreamSupplier {
        InputStream open() throws Exception;
    }

    private static final class ResourceItem {
        private final String name;
        private final long size;
        private final StreamSupplier supplier;

        private ResourceItem(String name, long size, StreamSupplier supplier) {
            this.name = name;
            this.size = size;
            this.supplier = supplier;
        }

        private String getName() {
            return name;
        }

        private long getSize() {
            return size;
        }

        private InputStream open() throws Exception {
            InputStream in = supplier.open();
            if (in == null) {
                throw new UnsatisfiedLinkError("无法打开资源: " + name);
            }
            return in;
        }
    }
}
