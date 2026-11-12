package com.chua.nativesmb.support;

import com.chua.common.support.nativeapi.SmbServerSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link SmbServerSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeSmbServerSpi implements SmbServerSpi {

    @Override
    public void loadLibrary() {
        RustSmbServerBridge.loadLibrary();
    }

    @Override
    public boolean isLoaded() {
        return RustSmbServerBridge.isLoaded();
    }

    @Override
    public long start(String bindAddr, int port, String shareName, String rootPath,
                      String user, String password) {
        return RustSmbServerBridge.start(bindAddr, port, shareName, rootPath, user, password);
    }

    @Override
    public void stop(long handle) {
        RustSmbServerBridge.stop(handle);
    }

    @Override
    public String[] listShares(long handle) {
        return RustSmbServerBridge.listShares(handle);
    }
}
