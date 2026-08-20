package com.chua.common.support.nativehttp.server;

import com.chua.common.support.network.http.HttpHeader;
import com.chua.common.support.network.http.HttpMethod;
import com.chua.common.support.network.server.request.AbstractServerRequest;

import java.util.Map;

/**
 * 基于共享内存信封的请求实现。
 *
 * @author CH
 * @since 4.0.0.42
 */
public class ShmServerRequest extends AbstractServerRequest {

    /** 请求方法 */
    private final HttpMethod method;

    /** 请求路径（含查询参数） */
    private final String uri;

    /** 请求体 */
    private final byte[] body;

    /**
     * 创建 ShmServerRequest 实例
     *
     * @param method 方法
     * @param path   路径
     * @param body   请求体
     */
    public ShmServerRequest(String method, String path, byte[] body) {
        this.method = parseMethod(method);
        this.uri = path;
        this.body = body;
    }

    /** 解析HttpMethod */
    private static HttpMethod parseMethod(String m) {
        try {
            return HttpMethod.valueOf(m.toUpperCase());
        } catch (IllegalArgumentException e) {
            return HttpMethod.GET;
        }
    }

    @Override
    /** 获取Uri */
    public String getUri() {
        return uri;
    }

    @Override
    /** 获取Path */
    public String getPath() {
        int idx = uri.indexOf('?');
        return idx >= 0 ? uri.substring(0, idx) : uri;
    }

    @Override
    /** 获取Method */
    public HttpMethod getMethod() {
        return method;
    }

    @Override
    /** 获取Header */
    public String getHeader(String name) {
        return null;
    }

    @Override
    /** 获取Headers */
    public HttpHeader getHeaders() {
        return HttpHeader.create();
    }

    @Override
    /** 获取Params */
    public Map<String, String> getParams() {
        int idx = uri.indexOf('?');
        if (idx < 0) {
            return Map.of();
        }
        Map<String, String> map = new java.util.LinkedHashMap<>();
        for (String pair : uri.substring(idx + 1).split("&")) {
            String[] kv = pair.split("=", 2);
            map.put(kv[0], kv.length > 1 ? kv[1] : "");
        }
        return map;
    }

    @Override
    /** 获取RemoteAddress */
    public String getRemoteAddress() {
        return "shm";
    }

    @Override
    /** 获取RemotePort */
    public int getRemotePort() {
        return 0;
    }

    @Override
    /** 读取Body */
    protected byte[] readBody() {
        return body;
    }
}
