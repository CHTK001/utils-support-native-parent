package com.chua.common.support.utils;

import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * Native 工具类：平台检测与库加载。
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NativeUtils {

    private NativeUtils() {
    }

    /**
     * 获取当前平台的目录名（如 windows-x86_64、linux-x86_64、osx-x86_64）
     */
    public static String getPlatformDir() {
        String os = System.getProperty("os.name").toLowerCase();
        String arch = System.getProperty("os.arch").toLowerCase();

        if (os.contains("win")) {
            return arch.equals("amd64") || arch.equals("x86_64") ? "windows-x86_64" : "windows-x86";
        } else if (os.contains("linux")) {
            return arch.equals("amd64") || arch.equals("x86_64") ? "linux-x86_64" : "linux-x86";
        } else if (os.contains("mac") || os.contains("osx")) {
            return arch.equals("amd64") || arch.equals("x86_64") ? "osx-x86_64" : "osx-x86";
        }
        return "unknown";
    }

    /**
     * 获取平台目录候选列表
     */
    public static String[] getPlatformDirCandidates() {
        return new String[]{
            getPlatformDir(),
            "windows-x86_64",
            "linux-x86_64",
            "osx-x86_64",
            "windows-x86",
            "linux-x86",
            "osx-x86"
        };
    }

    /**
     * 获取库文件名（添加 lib 前缀和 .dll/.so/.dylib 后缀）
     */
    public static String getLibraryFileName(String baseName) {
        return getLibraryFileName(baseName, true);
    }

    /**
     * 获取库文件名
     *
     * @param baseName 库基础名
     * @param addPrefix 是否添加 lib 前缀
     */
    public static String getLibraryFileName(String baseName, boolean addPrefix) {
        String prefix = addPrefix ? "lib" : "";
        String ext = getLibraryExtension();
        return prefix + baseName + ext;
    }

    /**
     * 获取动态库扩展名
     */
    public static String getLibraryExtension() {
        String os = System.getProperty("os.name").toLowerCase();
        if (os.contains("win")) {
            return ".dll";
        } else if (os.contains("linux")) {
            return ".so";
        } else if (os.contains("mac") || os.contains("osx")) {
            return ".dylib";
        }
        return ".so";
    }

    /**
     * 从路径加载库（内部方法，NativeLoader 调用）
     */
    public static void loadFromPathInternal(ClassLoader classLoader, String path) {
        System.load(path);
    }

    /**
     * 从类路径加载库
     */
    public static void loadFromClasspath(String resourceName) {
        try {
            java.net.URL url = NativeUtils.class.getClassLoader().getResource(resourceName);
            if (url == null) {
                throw new UnsatisfiedLinkError("找不到资源: " + resourceName);
            }
            java.io.File file = new java.io.File(url.toURI());
            System.load(file.getAbsolutePath());
        } catch (Exception e) {
            throw new RuntimeException("加载 native 库失败: " + resourceName, e);
        }
    }
}
