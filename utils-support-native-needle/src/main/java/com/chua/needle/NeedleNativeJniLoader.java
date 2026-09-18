package com.chua.needle;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;

import java.nio.file.Path;

/**
 * Needle 推理引擎原生库加载器。
 * 通过 NativeLoader 从 classpath 提取 needle native 动态库并加载。
 */
final class NeedleNativeJniLoader {

    private static volatile boolean loaded;

    /**
     * 构造方法，创建 NeedleNativeJniLoader 实例。
     */
    private NeedleNativeJniLoader() {
    }

    /**
     * 加载。
     */
    static void load() {
        if (loaded) {
            return;
        }
        synchronized (NeedleNativeJniLoader.class) {
            if (loaded) {
                return;
            }
            try {
                Path target = NativeUtils.tempRoot().resolve("chua_native_needle");
                NativeLoader.of("chua_native_needle")
                        .glob("*needle*")
                        .toTarget(target)
                        .load();
                loaded = true;
            } catch (Throwable e) {
                loaded = false;
                throw new RuntimeException("Failed to load chua_native_needle native library", e);
            }
        }
    }
}
