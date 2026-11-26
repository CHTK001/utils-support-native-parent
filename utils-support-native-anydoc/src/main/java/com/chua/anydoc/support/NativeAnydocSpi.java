package com.chua.anydoc.support;

import com.chua.common.support.nativeapi.AnydocSpi;
import com.chua.common.support.spi.annotations.Spi;

import java.nio.file.Path;

/**
 * {@link AnydocSpi} 的 native 实现。
 *
 * <p>只做静态门面到 SPI 契约的适配：原生加载、符号绑定与内存释放仍由
 * {@link AnydocBridge} 负责。</p>
 *
 * @author CH
 * @since 4.0.0.51
 */
@Spi("native")
public final class NativeAnydocSpi implements AnydocSpi {

    @Override
    public void load() {
        AnydocBridge.load();
    }

    @Override
    public boolean isLoaded() {
        return AnydocBridge.isLoaded();
    }

    @Override
    public Throwable getLoadError() {
        return AnydocBridge.getLoadError();
    }

    @Override
    public String version() {
        return AnydocBridge.isLoaded() ? AnydocBridge.version() : "";
    }

    @Override
    public String convertFile(Path file) {
        return AnydocBridge.convertFile(file);
    }

    @Override
    public String convertBytes(byte[] data, String format) {
        return AnydocBridge.convertBytes(data, format);
    }

    @Override
    public String lastError() {
        return AnydocBridge.lastError();
    }
}
