package com.chua.video.processor.support.bridge;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import java.nio.file.Path;

/**
 * VideoProcessor Rust native 库的 JNI 绑定。
 *
 * <p>声明 {@code transcodeToHls} 与 {@code getVersion} 两个原生方法，并在静态块中
 * 由 {@link NativeLoader} 从 classpath 的 {@code native/<平台>/} 目录抽取并加载
 * {@code video_processor} 动态库。</p>
 *
 * <p><b>包名必须与动态库中编译进的 JNI 符号一致</b>：本库导出的是
 * {@code Java_com_chua_video_processor_support_bridge_VideoProcessorBridge_*}，
 * 故即便本文件迁到 native 模块，{@code package} 声明也不能改，否则原生方法
 * 将无法解析（UnsatisfiedLinkError）。</p>
 *
 * <p>本文件原位于 {@code utils-support-video-processor-starter}，为让 JNI 绑定与
 * 动态库同住而迁入本模块；因全限定名保持不变，调用方无需改动。</p>
 *
 * @author CH
 * @since 4.0.0
 */

public class VideoProcessorBridge {

    /**
     * 图书馆_名称
    */
    private static final String LIBRARY_NAME = "video_processor";
    /**
     * 加载
    */
    private static volatile boolean loaded = false;
    /**
     * 加载错误
    */
    private static volatile Throwable loadError = null;

    static {
        try {
            String libFile = NativeUtils.getLibraryFileName(LIBRARY_NAME);
            NativeLoader loader = NativeLoader.of(LIBRARY_NAME)
                    .from(VideoProcessorBridge.class.getClassLoader())
                    .glob(libFile)
                    .toTarget(NativeUtils.tempRoot().resolve(LIBRARY_NAME));
            loader.load();
            loaded = true;
        } catch (Throwable e) {
            loadError = e;
            loaded = false;
        }
    }

    /**
     * 是否加载
     *
     * @return 是否加载的结果
     */
    public static boolean isLoaded() {
        return loaded;
    }

    /**
     * 获取加载记录错误
     *
     * @return 获取加载错误的结果
     */
    public static Throwable getLoadError() {
        return loadError;
    }

    /**
     * ensure加载
    */
    public static void ensureLoaded() {
        if (!loaded) {
            throw new UnsupportedOperationException(
                    "Rust VideoProcessor native library not loaded: " +
                            (loadError != null ? loadError.getMessage() : "unknown error"));
        }
    }

    /**
     * transcode转为hls
     *
     * @param inputPath 输入路径
     * @param outputDir 输出dir
     * @return transcode转为hls的结果
     */
    public static native boolean transcodeToHls(String inputPath, String outputDir);

    /**
     * 获取版本
     *
     * @return 获取版本的结果
     */
    public static native String getVersion();
}
