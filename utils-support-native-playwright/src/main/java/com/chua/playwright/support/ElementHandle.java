package com.chua.playwright.support;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * 对应 playwright-java 的 {@code ElementHandle}。持有 Rust 端的元素句柄。
 */
public class ElementHandle {

    final long handle;

    ElementHandle(long handle) {
        this.handle = handle;
    }

    public long handle() {
        return handle;
    }

    public void click() {
        Playwright.request("click", handle, null);
    }

    public void dblclick() {
        Playwright.request("dblclick", handle, null);
    }

    public void fill(String value) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("value", value);
        Playwright.request("fill", handle, p);
    }

    public String textContent() {
        return Playwright.asString(Playwright.requestData("textContent", handle, null));
    }

    public String innerText() {
        return Playwright.asString(Playwright.requestData("innerText", handle, null));
    }

    public String innerHTML() {
        return Playwright.asString(Playwright.requestData("innerHTML", handle, null));
    }

    public String getAttribute(String name) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("name", name);
        return Playwright.asString(Playwright.requestData("getAttribute", handle, p));
    }

    public void hover() {
        Playwright.request("hover", handle, null);
    }

    public byte[] screenshot() {
        Map<String, Object> d = Playwright.request("screenshot", handle, null);
        String b64 = (String) d.get("base64");
        return java.util.Base64.getDecoder().decode(b64);
    }

    public Object evaluate(String expression) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("expression", expression);
        return Playwright.requestData("evaluate", handle, p);
    }

    public ElementHandle querySelector(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        long h = Playwright.requestHandle("querySelector", handle, p);
        return new ElementHandle(h);
    }

    public void dispose() {
        Playwright.request("close", handle, null);
    }
}
