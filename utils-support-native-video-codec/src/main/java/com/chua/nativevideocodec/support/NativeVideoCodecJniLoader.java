package com.chua.nativevideocodec.support;

/**
 * Rust 支持的原生视频编码库加载器。
 *
 * @author CH
 * @since 4.0.0.42
 */
final class NativeVideoCodecJniLoader {

    /**
     * 库是否已加载
     */
    private static volatile boolean loaded;

    private NativeVideoCodecJniLoader() {
    }

    /**
     * 加载原生视频编码库。
     */
    static void load() {
        if (loaded) {
            return;
        }
        synchronized (NativeVideoCodecJniLoader.class) {
            if (loaded) {
                return;
            }
            String lib = System.mapLibraryName("chua-native-video-codec");
            System.loadLibrary(lib.replaceFirst("^lib", "").replaceAll("\\.[^.]+$", ""));
            loaded = true;
        }
    }
}
