package com.chua.common.support.nativehttp.server;

import com.chua.common.support.network.server.response.AbstractServerResponse;

import java.io.ByteArrayOutputStream;
import java.io.OutputStream;

/**
 * 基于共享内存信封的响应实现。
 *
 * <p>响应体在 {@code end()} 后由 {@link ShmHttpServer} 读取并通过
 * {@code rhb_send_response} 写回响应队列。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class ShmServerResponse extends AbstractServerResponse {

    /** 输出流缓冲 */
    private final ByteArrayOutputStream out = new ByteArrayOutputStream();

    @Override
    /** 获取OutputStream */
    public OutputStream getOutputStream() {
        return out;
    }

    @Override
    /** 写入Raw */
    public void writeRaw(byte[] bytes) {
        out.writeBytes(bytes);
        this.body = out.toByteArray();
    }

    @Override
    /** End */
    public void end() {
        super.end();
        // 仅通过 writeRaw 写入时，end 后回填 body 供上层读取
        if (this.body == null && out.size() > 0) {
            this.body = out.toByteArray();
        }
    }
}