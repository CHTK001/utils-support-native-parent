package com.chua.playwright.support;

import java.util.Map;

/**
 * 对应 playwright-java 的 {@code Response}。
 */
public class Response {

    private final int status;
    private final String url;

    Response(Map<String, Object> d) {
        this.status = d.get("status") == null ? 0 : ((Number) d.get("status")).intValue();
        this.url = (String) d.get("url");
    }

    public int status() {
        return status;
    }

    public String url() {
        return url;
    }
}
