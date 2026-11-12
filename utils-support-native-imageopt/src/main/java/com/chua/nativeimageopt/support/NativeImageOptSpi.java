package com.chua.nativeimageopt.support;

import com.chua.common.support.nativeapi.ImageOptSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link ImageOptSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeImageOptSpi implements ImageOptSpi {

    @Override
    public void load() {
        ImageOptBridge.load();
    }

    @Override
    public boolean isLoaded() {
        return ImageOptBridge.isLoaded();
    }

    @Override
    public String lastError() {
        return ImageOptBridge.lastError();
    }

    @Override
    public byte[] zlibCompress(byte[] data, int level) {
        return ImageOptBridge.zlibCompress(data, level);
    }

    @Override
    public byte[] zlibCompressBest(byte[] data, int level, int attempts) {
        return ImageOptBridge.zlibCompressBest(data, level, attempts);
    }

    @Override
    public Quantized quantize(byte[] rgba, int width, int height, int maxColors, int dither, int speed) {
        ImageOptBridge.Quantized quantized = ImageOptBridge.quantize(rgba, width, height,
                maxColors, dither, speed);
        return quantized == null ? null
                : new Quantized(quantized.palette(), quantized.colors(), quantized.indices());
    }
}
