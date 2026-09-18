package com.chua.nativeffmpeg.support;

/**
 * 基于 Rust cdylib 的原生 FFmpeg 门面。
 *
 * <p>所有方法通过 JNI 调用 Rust 原生 FFmpeg 封装库（ffmpeg_rust），
 * 支持 RTMP 推流/拉流、文件转码、截帧、拼接、媒体信息查询、
 * 旋转、水印与图片序列转视频等功能。</p>
 *
 * <p>原生库缺失或加载失败时，{@link #isLoaded()} 返回 {@code false}，
 * {@link #getLoadError()} 返回具体异常，调用方应据此降级处理。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NativeFFmpeg {

    static {
        try {
            NativeFFmpegJniLoader.load();
        } catch (Throwable ignored) {
            // 原生库缺失时降级：isLoaded() 返回 false，不阻断类加载
        }
    }

    /**
     * 构造方法，创建 NativeFFmpeg 实例。
     */
    private NativeFFmpeg() {
    }

    /**
     * 帧回调接口，推流/拉流回调模式下逐帧触发。
     */
    @FunctionalInterface
    public interface FrameCallback {

        /**
         * 帧回调。
         *
         * @param frameNumber 帧序号（从 0 开始）
         * @param timestampMs 帧时间戳（毫秒）
         * @param width 视频宽度
         * @param height 视频高度
         * @param codec 视频编码器名称
         * @param fps 帧率
         * @param keyFrame 是否为关键帧
         */
        void onFrame(long frameNumber, long timestampMs, int width, int height,
                     String codec, double fps, boolean keyFrame);
    }

    /**
     * 原生库是否加载成功。
     *
     * @return true 表示已加载
     */
    public static boolean isLoaded() {
        try {
            getVersion();
            return true;
        } catch (Throwable e) {
            return false;
        }
    }

    /**
     * 获取原生库加载失败原因。
     *
     * @return 加载失败异常，加载成功返回 null
     */
    public static Throwable getLoadError() {
        try {
            getVersion();
            return null;
        } catch (Throwable e) {
            return e;
        }
    }

    /**
     * 获取原生库版本号。
     *
     * @return 版本号
     */
    public static native String getVersion();

    // ---- 推流/拉流 ----

    /**
     * RTMP 推流（无回调）。
     *
     * @param inputUrl 输入文件 URL 或本地路径
     * @param streamUrl 推流地址
     * @param videoCodec 视频编码器，为空时使用默认值
     * @param audioCodec 音频编码器，为空时使用默认值
     * @param width 视频宽度，0 表示自动
     * @param height 视频高度，0 表示自动
     * @param fps 帧率，0 表示自动
     * @return 0 表示成功
     */
    public static native int pushStream(String inputUrl, String streamUrl,
                                        String videoCodec, String audioCodec,
                                        int width, int height, int fps);

    /**
     * RTMP 推流（带帧回调）。
     *
     * @param inputUrl 输入文件 URL 或本地路径
     * @param streamUrl 推流地址
     * @param videoCodec 视频编码器，为空时使用默认值
     * @param audioCodec 音频编码器，为空时使用默认值
     * @param width 视频宽度，0 表示自动
     * @param height 视频高度，0 表示自动
     * @param fps 帧率，0 表示自动
     * @param callback 帧回调
     * @return 0 表示成功
     */
    public static native int pushStreamWithCallback(String inputUrl, String streamUrl,
                                                    String videoCodec, String audioCodec,
                                                    int width, int height, int fps,
                                                    FrameCallback callback);

    /**
     * RTMP 拉流（无回调）。
     *
     * @param streamUrl 拉流地址
     * @param outputPath 输出文件路径
     * @param duration 拉流时长，0 表示不限制
     * @return 0 表示成功
     */
    public static native int pullStream(String streamUrl, String outputPath, double duration);

    /**
     * RTMP 拉流（带帧回调）。
     *
     * @param streamUrl 拉流地址
     * @param outputPath 输出文件路径
     * @param duration 拉流时长，0 表示不限制
     * @param callback 帧回调
     * @return 0 表示成功
     */
    public static native int pullStreamWithCallback(String streamUrl, String outputPath,
                                                    double duration, FrameCallback callback);

    /**
     * 获取媒体时长。
     *
     * @param inputUrl 输入文件 URL 或本地路径
     * @return 时长（秒），失败返回 -1
     */
    public static native double getDuration(String inputUrl);

    // ---- 文件转码 ----

    /**
     * 转码文件。
     *
     * @param inputUrl 输入文件路径
     * @param outputPath 输出文件路径
     * @param videoCodec 视频编码器，为空时使用默认值
     * @param audioCodec 音频编码器，为空时使用默认值
     * @param width 视频宽度，0 表示自动
     * @param height 视频高度，0 表示自动
     * @param fps 帧率，0 表示自动
     * @param startTime 开始时间（秒），0 表示从头开始
     * @param duration 转码时长（秒），0 表示全部
     * @param removeVideo 是否移除视频流
     * @param removeAudio 是否移除音频流
     * @return 0 表示成功
     */
    public static native int convertFile(String inputUrl, String outputPath,
                                         String videoCodec, String audioCodec,
                                         int width, int height, int fps,
                                         double startTime, double duration,
                                         boolean removeVideo, boolean removeAudio);

    /**
     * 截取某一帧为图片。
     *
     * @param inputUrl 输入文件路径
     * @param timestampMs 截取时间点（毫秒）
     * @param outputPath 输出图片路径
     * @return 0 表示成功
     */
    public static native int captureFrame(String inputUrl, long timestampMs, String outputPath);

    /**
     * 拼接多个文件。
     *
     * @param inputPaths 输入文件路径，多个以文件分隔符分隔
     * @param outputPath 输出文件路径
     * @return 0 表示成功
     */
    public static native int concatFiles(String inputPaths, String outputPath);

    /**
     * 获取媒体信息（JSON 格式）。
     *
     * @param inputUrl 输入文件 URL 或本地路径
     * @return JSON 字符串，失败返回 null
     */
    public static native String getMediaInfo(String inputUrl);

    // ---- 后处理 ----

    /**
     * 旋转视频（90/180/270 度）。
     *
     * @param inputUrl 输入文件路径
     * @param outputPath 输出文件路径
     * @param angle 旋转角度，仅支持 90/180/270
     * @return 0 表示成功
     */
    public static native int rotate(String inputUrl, String outputPath, int angle);

    /**
     * 添加水印。
     *
     * @param inputUrl 输入视频路径
     * @param watermarkPath 水印图片路径
     * @param outputPath 输出文件路径
     * @param x 水印 X 坐标
     * @param y 水印 Y 坐标
     * @return 0 表示成功
     */
    public static native int addWatermark(String inputUrl, String watermarkPath,
                                          String outputPath, int x, int y);

    // ---- 图片序列 ----

    /**
     * 将图片序列转为视频。
     *
     * @param imageDir 图片所在目录
     * @param outputPath 输出视频路径
     * @param fps 帧率
     * @param imagePattern 图片命名模式，如 "帧_%06d.jpg"
     * @return 0 表示成功
     */
    public static native int imagesToVideo(String imageDir, String outputPath,
                                           int fps, String imagePattern);
}
