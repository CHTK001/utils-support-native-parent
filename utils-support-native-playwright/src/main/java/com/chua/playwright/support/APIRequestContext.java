package com.chua.playwright.support;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * 对应 playwright-java 的 {@code APIRequestContext}。
 */
public class APIRequestContext {

    final long handle;

    APIRequestContext(long handle) {
        this.handle = handle;
    }

    public ApiResponse get(String url) {
        return fetch("apiGet", url, null);
    }

    public ApiResponse post(String url, Object body) {
        return fetch("apiPost", url, body);
    }

    public ApiResponse put(String url, Object body) {
        return fetch("apiPut", url, body);
    }

    public ApiResponse delete(String url) {
        return fetch("apiDelete", url, null);
    }

    private ApiResponse fetch(String action, String url, Object body) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("url", url);
        if (body != null) {
            p.put("body", body);
        }
        Map<String, Object> d = Playwright.request(action, handle, p);
        return new ApiResponse(d);
    }

    /**
     * 对应 playwright-java 的 {@code APIResponse}。
     */
    public static class ApiResponse {
        private final int status;
        private final String body;

        public ApiResponse(Map<String, Object> d) {
            this.status = d.get("status") == null ? 0 : ((Number) d.get("status")).intValue();
            this.body = (String) d.get("body");
        }

        public int status() {
            return status;
        }

        public String body() {
            return body;
        }
    }
}
