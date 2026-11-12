package com.chua.nativeturbojpeg.support;

import com.chua.common.support.nativeapi.TurboJpegSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link TurboJpegSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeTurboJpegSpi implements TurboJpegSpi {

    @Override
    public void load() {
        TurboJpegBridge.load();
    }

    @Override
    public boolean isLoaded() {
        return TurboJpegBridge.isLoaded();
    }

    @Override
    public Throwable getLoadError() {
        return TurboJpegBridge.getLoadError();
    }

    @Override
    public String version() {
        return TurboJpegBridge.version();
    }

    @Override
    public int[] probe(byte[] jpeg) {
        return TurboJpegBridge.probe(jpeg);
    }

    @Override
    public byte[] compress(byte[] pixels, int width, int height, int pitch, int pixelFormat,
                           int quality, int subsamp, int flags) {
        return TurboJpegBridge.compress(pixels, width, height, pitch, pixelFormat, quality,
                subsamp, flags);
    }

    @Override
    public Decoded decompress(byte[] jpeg, int pixelFormat, int flags) {
        TurboJpegBridge.Decoded decoded = TurboJpegBridge.decompress(jpeg, pixelFormat, flags);
        return decoded == null ? null : new Decoded(decoded.pixels(), decoded.width(),
                decoded.height(), decoded.pitch());
    }
}
