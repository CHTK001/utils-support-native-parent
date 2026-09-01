package com.chua.nativevideocodec.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import java.nio.file.Path;

/**
 * Rust 视频编解码器 JNI 加载器。
 * 通过 NativeLoader 从 classpath 提取 DLL 并加载。
 */
final class NativeVideoCodecJniLoader {

    private static volatile boolean loaded;

    private NativeVideoCodecJniLoader() {
    }

    static void load() {
        if (loaded) {
            return;
        }
        synchronized (NativeVideoCodecJniLoader.class) {
            if (loaded) {
                return;
            }
            try {
                Path target = NativeUtils.tempRoot().resolve("chua_native_video_codec");
                NativeLoader.of("chua_native_video_codec")
                        .glob("chua_native_video_codec.dll")
                        .toTarget(target)
                        .load();
                loaded = true;
            } catch (Throwable e) {
                loaded = false;
                throw new RuntimeException("Failed to load chua_native_video_codec native library", e);
            }
        }
    }
}
