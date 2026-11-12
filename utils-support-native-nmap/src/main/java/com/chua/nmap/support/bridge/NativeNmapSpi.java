package com.chua.nmap.support.bridge;

import com.chua.common.support.nativeapi.NmapSpi;
import com.chua.common.support.spi.annotations.Spi;

/**
 * {@link NmapSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeNmapSpi implements NmapSpi {

    @Override
    public boolean isLoaded() {
        return RustNmapBridge.isLoaded();
    }

    @Override
    public void ensureLoaded() {
        RustNmapBridge.ensureLoaded();
    }

    @Override
    public int scanSingleTcpPort(String host, int port, int timeout) {
        return RustNmapBridge.scanSingleTcpPort(host, port, timeout);
    }

    @Override
    public String scanTcpPortRange(String host, int startPort, int endPort, int timeout, int concurrency) {
        return RustNmapBridge.scanTcpPortRange(host, startPort, endPort, timeout, concurrency);
    }

    @Override
    public String pingHost(String host, int timeout) {
        return RustNmapBridge.pingHost(host, timeout);
    }

    @Override
    public String scanSubnet(String subnet, int timeout, int concurrency) {
        return RustNmapBridge.scanSubnet(subnet, timeout, concurrency);
    }

    @Override
    public String detectService(String host, int port, int timeout) {
        return RustNmapBridge.detectService(host, port, timeout);
    }

    @Override
    public String getVersion() {
        return RustNmapBridge.getVersion();
    }
}
