package com.chua.nativemetrics.support;

import com.chua.common.support.nativeapi.MetricsNativeSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link MetricsNativeSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeMetricsSpi implements MetricsNativeSpi {

    /**
     * 原生库实例；加载失败时为 {@code null}
     */
    private final MetricsNativeLibrary delegate = MetricsNativeLibrary.create();

    @Override
    public boolean available() {
        return delegate != null;
    }

    @Override
    public void start(long intervalMs) {
        require().start(intervalMs);
    }

    @Override
    public String poll() {
        return require().poll();
    }

    @Override
    public void close() {
        if (delegate != null) {
            delegate.close();
        }
    }

    private MetricsNativeLibrary require() {
        if (delegate == null) {
            throw new IllegalStateException("metrics_native 原生库未加载");
        }
        return delegate;
    }
}
