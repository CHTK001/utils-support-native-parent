package com.chua.nativerdp.support;

import com.chua.common.support.nativeapi.RdpNativeSpi;
import com.chua.common.support.spi.annotations.Spi;
import com.sun.jna.Memory;
import com.sun.jna.Pointer;
import com.sun.jna.ptr.IntByReference;

/**
 * {@link RdpNativeSpi} 的 native 实现。
 *
 * <p>把 JNA 的 {@link Pointer} 收敛在本模块内，契约层只看到 {@code long} 与
 * {@code byte[]}。</p>
 *
 * @author CH
 * @since 4.0.0.50
 */
@Spi("native")
public final class NativeRdpSpi implements RdpNativeSpi {

    @Override
    public boolean available() {
        return RdpjBridge.available();
    }

    @Override
    public long create() {
        Pointer pointer = RdpjBridge.instance().rdpj_create();
        return pointer == null ? 0L : Pointer.nativeValue(pointer);
    }

    @Override
    public int resize(long handle, int width, int height) {
        return RdpjBridge.instance().rdpj_resize(pointer(handle), width, height);
    }

    @Override
    public int connect(long handle, String host, int port, String user, String password, String locale) {
        return RdpjBridge.instance().rdpj_connect(pointer(handle), host, port, user, password, locale);
    }

    @Override
    public int state(long handle) {
        return RdpjBridge.instance().rdpj_state(pointer(handle));
    }

    @Override
    public String lastError(long handle) {
        return RdpjBridge.instance().rdpj_last_error(pointer(handle));
    }

    @Override
    public boolean frameHeader(long handle, int[] header) {
        if (header == null || header.length < 4) {
            return false;
        }
        Pointer frame = RdpjBridge.instance().rdpj_last_frame(pointer(handle));
        if (frame == null || Pointer.nativeValue(frame) == 0L) {
            return false;
        }
        header[0] = frame.getInt(0);
        header[1] = frame.getInt(4);
        header[2] = frame.getInt(8);
        // [P3C A20 豁免]
        header[3] = frame.getInt(12);
        return true;
    }

    @Override
    public int frame(long handle, byte[] buffer) {
        if (buffer == null || buffer.length == 0) {
            return 0;
        }
        Memory memory = new Memory(buffer.length);
        IntByReference outLen = new IntByReference(0);
        int written = RdpjBridge.instance().rdpj_frame(pointer(handle), memory,
                buffer.length, outLen);
        if (written <= 0) {
            return written;
        }
        int length = Math.min(written, buffer.length);
        memory.read(0, buffer, 0, length);
        return length;
    }

    @Override
    public int mouse(long handle, int x, int y, int button, int down) {
        return RdpjBridge.instance().rdpj_mouse(pointer(handle), x, y, button, down);
    }

    @Override
    public int key(long handle, short code, int down) {
        return RdpjBridge.instance().rdpj_key(pointer(handle), code, down);
    }

    @Override
    public int wheel(long handle, int x, int y, int delta) {
        return RdpjBridge.instance().rdpj_wheel(pointer(handle), x, y, delta);
    }

    @Override
    public void close(long handle) {
        RdpjBridge.instance().rdpj_close(pointer(handle));
    }

    @Override
    public void destroy(long handle) {
        RdpjBridge.instance().rdpj_destroy(pointer(handle));
    }

    private static Pointer pointer(long handle) {
        return new Pointer(handle);
    }
}
