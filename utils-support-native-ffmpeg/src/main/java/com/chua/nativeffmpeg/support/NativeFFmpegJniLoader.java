package com.chua.nativeffmpeg.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import java.nio.file.Path;

/**
 * Rust FFmpeg JNI 加载器。
 * 通过 NativeLoader 从 classpath 提取动态库并加载。
 */
final class NativeFFmpegJniLoader {

    private static volatile boolean loaded;

    private NativeFFmpegJniLoader() {
    }

    static void load() {
        if (loaded) {
            return;
        }
        synchronized (NativeFFmpegJniLoader.class) {
            if (loaded) {
                return;
            }
            try {
                Path target = NativeUtils.tempRoot().resolve("chua_native_ffmpeg");
                NativeLoader.of("chua_native_ffmpeg")
                        .glob("*ffmpeg_rust*")
                        .toTarget(target)
                        .load();
                loaded = true;
            } catch (Throwable e) {
                loaded = false;
                throw new RuntimeException("Failed to load chua_native_ffmpeg native library", e);
            }
        }
    }
}
