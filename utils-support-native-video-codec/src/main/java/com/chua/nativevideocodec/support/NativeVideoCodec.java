package com.chua.nativevideocodec.support;

/**
 * 基于 Rust cdylib 的原生 H.264/H.265/H.266 编码器/解码器门面。
 *
 * <p>所有编码方法接受 BGR24 字节数组并返回编码后的字节数组。
 * 所有解码方法接受编码数据包并返回解码后的字节数组。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NativeVideoCodec {

    static {
        NativeVideoCodecJniLoader.load();
    }

    private NativeVideoCodec() {
    }

    // ---- 编码器生命周期 ----

    /**
     * 创建 H.264 编码器。
     *
     * @param width 视频宽度
     * @param height 视频高度
     * @param fps 帧率
     * @param quality 画质
     * @param preset 编码 preset
     * @param profile 编码 profile
     * @return 编码器句柄
     */
    public static native long h264EncoderCreate(int width, int height, int fps, int quality, int preset, int profile);

    /**
     * H.264 编码一帧。
     *
     * @param encoder 编码器句柄
     * @param bgr24 BGR24 格式像素数据
     * @param width 视频宽度
     * @param height 视频高度
     * @return 编码后的字节数组
     */
    public static native byte[] h264Encode(long encoder, byte[] bgr24, int width, int height);

    /**
     * 释放 H.264 编码器。
     *
     * @param encoder 编码器句柄
     */
    public static native void h264EncoderFree(long encoder);

    /**
     * 创建 H.265 编码器。
     *
     * @param width 视频宽度
     * @param height 视频高度
     * @param fps 帧率
     * @param quality 画质
     * @param preset 编码 preset
     * @param profile 编码 profile
     * @return 编码器句柄
     */
    public static native long h265EncoderCreate(int width, int height, int fps, int quality, int preset, int profile);

    /**
     * H.265 编码一帧。
     *
     * @param encoder 编码器句柄
     * @param bgr24 BGR24 格式像素数据
     * @param width 视频宽度
     * @param height 视频高度
     * @return 编码后的字节数组
     */
    public static native byte[] h265Encode(long encoder, byte[] bgr24, int width, int height);

    /**
     * 释放 H.265 编码器。
     *
     * @param encoder 编码器句柄
     */
    public static native void h265EncoderFree(long encoder);

    /**
     * 创建 H.266 编码器。
     *
     * @param width 视频宽度
     * @param height 视频高度
     * @param fps 帧率
     * @param quality 画质
     * @param preset 编码 preset
     * @param profile 编码 profile
     * @return 编码器句柄
     */
    public static native long h266EncoderCreate(int width, int height, int fps, int quality, int preset, int profile);

    /**
     * H.266 编码一帧。
     *
     * @param encoder 编码器句柄
     * @param bgr24 BGR24 格式像素数据
     * @param width 视频宽度
     * @param height 视频高度
     * @return 编码后的字节数组
     */
    public static native byte[] h266Encode(long encoder, byte[] bgr24, int width, int height);

    /**
     * 释放 H.266 编码器。
     *
     * @param encoder 编码器句柄
     */
    public static native void h266EncoderFree(long encoder);

    // ---- 解码器生命周期 ----

    /**
     * 创建 H.264 解码器。
     *
     * @param width 视频宽度
     * @param height 视频高度
     * @return 解码器句柄
     */
    public static native long h264DecoderCreate(int width, int height);

    /**
     * H.264 解码一包数据。
     *
     * @param decoder 解码器句柄
     * @param packet 编码数据包
     * @param packetLen 数据包长度
     * @return 解码后的字节数组
     */
    public static native byte[] h264Decode(long decoder, byte[] packet, int packetLen);

    /**
     * 释放 H.264 解码器。
     *
     * @param decoder 解码器句柄
     */
    public static native void h264DecoderFree(long decoder);

    /**
     * 创建 H.265 解码器。
     *
     * @param width 视频宽度
     * @param height 视频高度
     * @return 解码器句柄
     */
    public static native long h265DecoderCreate(int width, int height);

    /**
     * H.265 解码一包数据。
     *
     * @param decoder 解码器句柄
     * @param packet 编码数据包
     * @param packetLen 数据包长度
     * @return 解码后的字节数组
     */
    public static native byte[] h265Decode(long decoder, byte[] packet, int packetLen);

    /**
     * 释放 H.265 解码器。
     *
     * @param decoder 解码器句柄
     */
    public static native void h265DecoderFree(long decoder);

    /**
     * 创建 H.266 解码器。
     *
     * @param width 视频宽度
     * @param height 视频高度
     * @return 解码器句柄
     */
    public static native long h266DecoderCreate(int width, int height);

    /**
     * H.266 解码一包数据。
     *
     * @param decoder 解码器句柄
     * @param packet 编码数据包
     * @param packetLen 数据包长度
     * @return 解码后的字节数组
     */
    public static native byte[] h266Decode(long decoder, byte[] packet, int packetLen);

    /**
     * 释放 H.266 解码器。
     *
     * @param decoder 解码器句柄
     */
    public static native void h266DecoderFree(long decoder);

    /**
     * 获取原生库版本号。
     *
     * @return 版本号
     */
    public static native int getVersion();
}
