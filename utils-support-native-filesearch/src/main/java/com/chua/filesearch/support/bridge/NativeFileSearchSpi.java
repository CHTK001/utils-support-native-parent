package com.chua.filesearch.support.bridge;

import com.chua.common.support.nativeapi.FileSearchSpi;
import com.chua.common.support.spi.annotations.Spi;

import java.util.function.Consumer;

/**
 * {@link FileSearchSpi} 的 native 实现。
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeFileSearchSpi implements FileSearchSpi {

    @Override
    public void loadLibrary() {
        RustFileSearchBridge.loadLibrary();
    }

    @Override
    public boolean isLoaded() {
        return RustFileSearchBridge.isLoaded();
    }

    @Override
    public Throwable getLoadError() {
        return RustFileSearchBridge.getLoadError();
    }

    @Override
    public String getVersion() {
        return RustFileSearchBridge.getVersion();
    }

    @Override
    public void cancel() {
        RustFileSearchBridge.cancel();
    }

    @Override
    public int searchByName(String root, String pattern, int max, Consumer<Result> callback) {
        return RustFileSearchBridge.searchByName(root, pattern, max, toNative(callback));
    }

    @Override
    public int getTree(String root, int depth, int max, Consumer<Result> callback) {
        return RustFileSearchBridge.getTree(root, depth, max, toNative(callback));
    }

    @Override
    public int searchBySize(String root, long minSize, long maxSize, int max,
                            Consumer<Result> callback) {
        return RustFileSearchBridge.searchBySize(root, minSize, maxSize, max, toNative(callback));
    }

    @Override
    public int searchByPath(String root, String pattern, int max, Consumer<Result> callback) {
        return RustFileSearchBridge.searchByPath(root, pattern, max, toNative(callback));
    }

    private static Consumer<RustFileSearchBridge.FileResultData> toNative(Consumer<Result> callback) {
        if (callback == null) {
            return null;
        }
        return data -> callback.accept(new Result(data.path(), data.size(), data.lastModified(),
                data.isDirectory(), data.extension(), data.attributes(), data.usnRecordId(),
                data.parentFileId(), data.allocatedSize()));
    }
}
