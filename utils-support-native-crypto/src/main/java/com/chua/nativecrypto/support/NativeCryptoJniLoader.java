package com.chua.nativecrypto.support;

/**
 * Rust 支持的原生加密库加载器。
 *
 * @author CH
 * @since 4.0.0.42
 */
final class NativeCryptoJniLoader {

    /**
     * 库是否已加载
     */
    private static volatile boolean loaded;

    private NativeCryptoJniLoader() {
    }

    /**
     * 加载原生加密库。
     */
    static void load() {
        if (loaded) {
            return;
        }
        synchronized (NativeCryptoJniLoader.class) {
            if (loaded) {
                return;
            }
            // 根据平台拼接库名，例如 "chua-native-crypto" 或 "libchua_native_crypto.so"。
            String lib = System.mapLibraryName("chua-native-crypto");
            System.loadLibrary(lib.replaceFirst("^lib", "").replaceAll("\\.[^.]+$", ""));
            loaded = true;
        }
    }
}
