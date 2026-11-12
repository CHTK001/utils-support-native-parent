package com.chua.nativeffmpeg.support;

import com.chua.common.support.nativeapi.FFmpegNativeSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link FFmpegNativeSpi} 的 native 实现。
 *
 * <p>JNI 绑定类 {@link NativeFFmpeg} 的帧回调类型保持原样，本类只做契约适配。</p>
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeFFmpegSpiImpl implements FFmpegNativeSpi {

    @Override
    public Throwable getLoadError() {
        return NativeFFmpeg.getLoadError();
    }

    @Override
    public String getVersion() {
        return NativeFFmpeg.getVersion();
    }

    @Override
    public int pushStream(String inputUrl, String streamUrl, String videoCodec, String audioCodec,
                          int width, int height, int fps) {
        return NativeFFmpeg.pushStream(inputUrl, streamUrl, videoCodec, audioCodec, width, height, fps);
    }

    @Override
    public int pushStreamWithCallback(String inputUrl, String streamUrl, String videoCodec,
                                      String audioCodec, int width, int height, int fps,
                                      FrameCallback callback) {
        return NativeFFmpeg.pushStreamWithCallback(inputUrl, streamUrl, videoCodec, audioCodec,
                width, height, fps, toNative(callback));
    }

    @Override
    public int pullStream(String streamUrl, String outputPath, double duration) {
        return NativeFFmpeg.pullStream(streamUrl, outputPath, duration);
    }

    @Override
    public int pullStreamWithCallback(String streamUrl, String outputPath, double duration,
                                      FrameCallback callback) {
        return NativeFFmpeg.pullStreamWithCallback(streamUrl, outputPath, duration, toNative(callback));
    }

    @Override
    public double getDuration(String inputUrl) {
        return NativeFFmpeg.getDuration(inputUrl);
    }

    @Override
    public int convertFile(String inputUrl, String outputPath, String videoCodec, String audioCodec,
                           int width, int height, int fps, double startTime, double duration,
                           boolean removeVideo, boolean removeAudio) {
        return NativeFFmpeg.convertFile(inputUrl, outputPath, videoCodec, audioCodec, width, height,
                fps, startTime, duration, removeVideo, removeAudio);
    }

    @Override
    public int captureFrame(String inputUrl, long timestampMs, String outputPath) {
        return NativeFFmpeg.captureFrame(inputUrl, timestampMs, outputPath);
    }

    @Override
    public int concatFiles(String inputPaths, String outputPath) {
        return NativeFFmpeg.concatFiles(inputPaths, outputPath);
    }

    @Override
    public String getMediaInfo(String inputUrl) {
        return NativeFFmpeg.getMediaInfo(inputUrl);
    }

    @Override
    public int rotate(String inputUrl, String outputPath, int angle) {
        return NativeFFmpeg.rotate(inputUrl, outputPath, angle);
    }

    @Override
    public int addWatermark(String inputUrl, String watermarkPath, String outputPath, int x, int y) {
        return NativeFFmpeg.addWatermark(inputUrl, watermarkPath, outputPath, x, y);
    }

    @Override
    public int imagesToVideo(String imageDir, String outputPath, int fps, String imagePattern) {
        return NativeFFmpeg.imagesToVideo(imageDir, outputPath, fps, imagePattern);
    }

    private static NativeFFmpeg.FrameCallback toNative(FrameCallback callback) {
        if (callback == null) {
            return null;
        }
        return (frameNumber, timestampMs, width, height, codec, fps, keyFrame) ->
                callback.onFrame(frameNumber, timestampMs, width, height, codec, fps, keyFrame);
    }
}
