package com.chua.nativesqlite.support;

import com.chua.common.support.nativeapi.SqliteHookSpi;
import com.chua.common.support.spi.annotations.Spi;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandle;

/**
 * {@link SqliteHookSpi} 的 native 实现。
 *
 * <p>只做静态绑定层到 SPI 契约的适配，原生加载与符号绑定仍由
 * {@link SqliteHookBridge} 负责。</p>
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeSqliteHookSpi implements SqliteHookSpi {

    @Override
    public boolean isAvailable() {
        return SqliteHookBridge.isAvailable();
    }

    @Override
    public MethodHandle hookOpen() {
        return SqliteHookBridge.hookOpen();
    }

    @Override
    public MethodHandle hookPoll() {
        return SqliteHookBridge.hookPoll();
    }

    @Override
    public MethodHandle hookExec() {
        return SqliteHookBridge.hookExec();
    }

    @Override
    public MethodHandle hookClose() {
        return SqliteHookBridge.hookClose();
    }

    @Override
    public MethodHandle hookOpenAsync() {
        return SqliteHookBridge.hookOpenAsync();
    }

    @Override
    public MethodHandle hookExecAsync() {
        return SqliteHookBridge.hookExecAsync();
    }

    @Override
    public MethodHandle hookCloseAsync() {
        return SqliteHookBridge.hookCloseAsync();
    }

    @Override
    public MemorySegment eventCallbackStub(MethodHandle target, Arena arena) {
        return SqliteHookBridge.eventCallbackStub(target, arena);
    }
}
