package com.chua.playwright.support;

import java.util.Map;

/**
 * 对应 playwright-java 的 {@code Browser}。持有 Rust 端的 browser 句柄。
 */
public class Browser {

    final long handle;

    Browser(long handle) {
        this.handle = handle;
    }

    public long handle() {
        return handle;
    }

    public BrowserContext newContext() {
        return newContext(null);
    }

    public BrowserContext newContext(Map<String, Object> options) {
        long h = Playwright.requestHandle("newContext", handle, options);
        return new BrowserContext(h);
    }

    public Page newPage() {
        long h = Playwright.requestHandle("newPage", handle, null);
        return new Page(h);
    }

    public void close() {
        Playwright.request("close", handle, null);
    }
}
