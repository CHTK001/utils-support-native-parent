package com.chua.playwright.support.bridge;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/**
 * 加载 Rust 编译的动态库（rust_playwright.{dll,so,dylib}）并提供 JNI 入口。
 * 动态库随 jar 打包在 resources/native/{platform}/ 下，运行期抽取到临时目录后加载。
 */
public final class PlaywrightNative {

    private static volatile boolean loaded = false;

    private PlaywrightNative() {
    }

    public static synchronized void ensureLoaded() {
        if (loaded) {
            return;
        }
        loadFromResource();
        loaded = true;
    }

    private static void loadFromResource() {
        String libName;
        String platformDir;
        String os = System.getProperty("os.name", "").toLowerCase();
        String arch = System.getProperty("os.arch", "").toLowerCase();
        if (os.contains("win")) {
            libName = "rust_playwright.dll";
            platformDir = "windows-x86_64";
        } else if (os.contains("linux")) {
            libName = "librust_playwright.so";
            platformDir = arch.contains("aarch") ? "linux-aarch64" : "linux-x86_64";
        } else if (os.contains("mac") || os.contains("darwin")) {
            libName = "librust_playwright.dylib";
            platformDir = arch.contains("aarch") ? "darwin-aarch64" : "darwin-x86_64";
        } else {
            throw new UnsupportedOperationException("不支持的平台: " + os + " " + arch);
        }

        String resPath = "/native/" + platformDir + "/" + libName;
        try (InputStream in = PlaywrightNative.class.getResourceAsStream(resPath)) {
            if (in == null) {
                throw new IllegalStateException(
                        "找不到 native 库: " + resPath + "，请先进入 src/main/rust 执行 ./build.sh 构建");
            }
            Path tmp = Files.createTempFile("rust_playwright_", "_" + libName);
            Files.copy(in, tmp, StandardCopyOption.REPLACE_EXISTING);
            tmp.toFile().deleteOnExit();
            System.load(tmp.toAbsolutePath().toString());
        } catch (IOException e) {
            throw new RuntimeException("加载 native 库失败", e);
        }
    }

    /** 执行 JSON 命令，返回 JSON 结果字符串。 */
    public static native String execute(String command);

    /** 返回 Rust 侧库版本。 */
    public static native String getVersion();

    /** 库是否可用。 */
    public static native boolean isAvailable();
}
