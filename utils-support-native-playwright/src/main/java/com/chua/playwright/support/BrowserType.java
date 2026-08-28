package com.chua.playwright.support;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * 对应 playwright-java 的 {@code BrowserType}。仅支持 chromium。
 */
public class BrowserType {

    private final String name;

    BrowserType(String name) {
        this.name = name;
    }

    public String name() {
        return name;
    }

    public Browser launch() {
        return launch(null);
    }

    public Browser launch(Map<String, Object> options) {
        Map<String, Object> params = new LinkedHashMap<>();
        if (options != null) {
            if (options.containsKey("headless")) {
                params.put("headless", options.get("headless"));
            }
            if (options.containsKey("executablePath")) {
                params.put("executablePath", options.get("executablePath"));
            }
            if (options.containsKey("args")) {
                params.put("args", options.get("args"));
            }
        }
        long h = Playwright.requestHandle("launch", null, params);
        return new Browser(h);
    }
}
