package com.chua.playwright.support;

/**
 * 对应 playwright-java 的 {@code BrowserContext}。
 */
public class BrowserContext {

    final long handle;

    BrowserContext(long handle) {
        this.handle = handle;
    }

    public long handle() {
        return handle;
    }

    public Page newPage() {
        long h = Playwright.requestHandle("newPage", handle, null);
        return new Page(h);
    }

    public void close() {
        Playwright.request("close", handle, null);
    }
}
