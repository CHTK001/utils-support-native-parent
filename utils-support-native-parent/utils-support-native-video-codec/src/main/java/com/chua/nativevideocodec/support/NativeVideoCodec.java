package com.chua.nativevideocodec.support;

/**
 * 视频编解码原生库 JNI 桥接类。
 *
 * <p>加载 native_video_codec 动态库，提供 H.264/H.265 编解码能力。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NativeVideoCodec {

    private static boolean loaded;

    static {
        try {
            com.chua.common.support.utils.NativeLoader.of("native-video-codec")
                    .toTarget(System.getProperty("java.io.tmpdir") + "/native-video-codec")
                    .glob("*native_video_codec*")
                    .load();
            loaded = true;
        } catch (Exception e) {
            System.err.println("[NativeVideoCodec] 加载失败: " + e.getMessage());
            loaded = false;
        }
    }

    private NativeVideoCodec() {
    }

    /**
     * 检查原生库是否已加载。
     *
     * @return true 表示已加载
     */
    public static boolean isLoaded() {
        return loaded;
    }

    public static native String getVersion();
    public static native long h264EncoderCreate(int width, int height, int fps, int quality, int bFrame, int threadCount);
    public static native byte[] h264Encode(long encoder, byte[] bgr24, int width, int height);
    public static native void h264EncoderFree(long encoder);
    public static native long h264DecoderCreate(int width, int height);
    public static native byte[] h264Decode(long decoder, byte[] packet, int length);
    public static native void h264DecoderFree(long decoder);

    public static native long h265EncoderCreate(int width, int height, int fps, int quality, int bFrame, int threadCount);
    public static native byte[] h265Encode(long encoder, byte[] bgr24, int width, int height);
    public static native void h265EncoderFree(long encoder);
    public static native long h265DecoderCreate(int width, int height);
    public static native byte[] h265Decode(long decoder, byte[] packet, int length);
    public static native void h265DecoderFree(long decoder);

    public static native long h266EncoderCreate(int width, int height, int fps, int quality, int bFrame, int threadCount);
    public static native byte[] h266Encode(long encoder, byte[] bgr24, int width, int height);
    public static native void h266EncoderFree(long encoder);
    public static native long h266DecoderCreate(int width, int height);
    public static native byte[] h266Decode(long decoder, byte[] packet, int length);
    public static native void h266DecoderFree(long decoder);
}