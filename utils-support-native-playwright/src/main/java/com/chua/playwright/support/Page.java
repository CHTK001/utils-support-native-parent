package com.chua.playwright.support;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * 对应 playwright-java 的 {@code Page}。持有 Rust 端的页面句柄。
 * 方法命名与 playwright-java 对齐，返回值尽量类型化（基本类型 / Response / ElementHandle）。
 */
public class Page {

    final long handle;

    Page(long handle) {
        this.handle = handle;
    }

    public long handle() {
        return handle;
    }

    // ======================== 导航 ========================

    public Response goto(String url) {
        return goto(url, null);
    }

    public Response goto(String url, Map<String, Object> options) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("url", url);
        if (options != null) {
            p.putAll(options);
        }
        Map<String, Object> d = Playwright.request("goto", handle, p);
        return new Response(d);
    }

    public Response reload() {
        return reload(null);
    }

    public Response reload(Map<String, Object> options) {
        Map<String, Object> d = Playwright.request("reload", handle, options);
        return new Response(d);
    }

    public Response goBack() {
        Object d = Playwright.requestData("goBack", handle, null);
        if (d == null || !(d instanceof Map)) {
            return null;
        }
        return new Response((Map<String, Object>) d);
    }

    public Response goForward() {
        Object d = Playwright.requestData("goForward", handle, null);
        if (d == null || !(d instanceof Map)) {
            return null;
        }
        return new Response((Map<String, Object>) d);
    }

    // ======================== 页面信息 ========================

    public String title() {
        return Playwright.asString(Playwright.requestData("title", handle, null));
    }

    public String url() {
        return Playwright.asString(Playwright.requestData("url", handle, null));
    }

    // ======================== 视口 ========================

    public void setViewportSize(int width, int height) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("width", width);
        p.put("height", height);
        Playwright.request("setViewportSize", handle, p);
    }

    // ======================== 交互 ========================

    public void click(String selector) {
        click(selector, null);
    }

    public void click(String selector, Map<String, Object> options) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        if (options != null) {
            p.putAll(options);
        }
        Playwright.request("click", handle, p);
    }

    public void dblclick(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        Playwright.request("dblclick", handle, p);
    }

    public void fill(String selector, String value) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("value", value);
        Playwright.request("fill", handle, p);
    }

    public void type(String selector, String text) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("value", text);
        Playwright.request("type", handle, p);
    }

    public void press(String selector, String key) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("key", key);
        Playwright.request("press", handle, p);
    }

    public void check(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        Playwright.request("check", handle, p);
    }

    public void uncheck(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        Playwright.request("uncheck", handle, p);
    }

    public void hover(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        Playwright.request("hover", handle, p);
    }

    public List<String> selectOption(String selector, String... values) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("values", values);
        Object d = Playwright.requestData("selectOption", handle, p);
        if (d instanceof List) {
            return (List<String>) d;
        }
        return new ArrayList<>();
    }

    // ======================== 读取内容 ========================

    public String textContent(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        return Playwright.asString(Playwright.requestData("textContent", handle, p));
    }

    public String innerText(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        return Playwright.asString(Playwright.requestData("innerText", handle, p));
    }

    public String innerHTML(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        return Playwright.asString(Playwright.requestData("innerHTML", handle, p));
    }

    public String getAttribute(String selector, String name) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("name", name);
        return Playwright.asString(Playwright.requestData("getAttribute", handle, p));
    }

    public String inputValue(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        return Playwright.asString(Playwright.requestData("inputValue", handle, p));
    }

    // ======================== 截图 ========================

    public byte[] screenshot() {
        return screenshot(null);
    }

    public byte[] screenshot(Map<String, Object> options) {
        Map<String, Object> d = Playwright.request("screenshot", handle, options);
        String b64 = (String) d.get("base64");
        return java.util.Base64.getDecoder().decode(b64);
    }

    // ======================== 脚本执行 ========================

    public Object evaluate(String expression) {
        return evaluate(expression, null);
    }

    public Object evaluate(String expression, Object arg) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("expression", expression);
        if (arg != null) {
            p.put("arg", arg);
        }
        return Playwright.requestData("evaluate", handle, p);
    }

    // ======================== 元素查询 ========================

    public ElementHandle querySelector(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        long h = Playwright.requestHandle("querySelector", handle, p);
        return new ElementHandle(h);
    }

    public List<ElementHandle> querySelectorAll(String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        Map<String, Object> d = Playwright.request("querySelectorAll", handle, p);
        List<Long> ids = (List<Long>) d.get("handles");
        List<ElementHandle> result = new ArrayList<>();
        if (ids != null) {
            for (Long id : ids) {
                result.add(new ElementHandle(id));
            }
        }
        return result;
    }

    public void waitForSelector(String selector) {
        waitForSelector(selector, null);
    }

    public void waitForSelector(String selector, Map<String, Object> options) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        if (options != null) {
            p.putAll(options);
        }
        Playwright.request("waitForSelector", handle, p);
    }

    // ======================== 关闭 ========================

    public void close() {
        Playwright.request("close", handle, null);
    }
}
