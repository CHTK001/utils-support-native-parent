package com.chua.video.processor.support.bridge;

import com.chua.common.support.nativeapi.VideoProcessorSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link VideoProcessorSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeVideoProcessorSpi implements VideoProcessorSpi {

    @Override
    public boolean isLoaded() {
        return VideoProcessorBridge.isLoaded();
    }

    @Override
    public void ensureLoaded() {
        VideoProcessorBridge.ensureLoaded();
    }

    @Override
    public boolean transcodeToHls(String inputPath, String outputDir) {
        return VideoProcessorBridge.transcodeToHls(inputPath, outputDir);
    }

    @Override
    public String getVersion() {
        return VideoProcessorBridge.getVersion();
    }

    @Override
    public Throwable getLoadError() {
        return VideoProcessorBridge.getLoadError();
    }
}
