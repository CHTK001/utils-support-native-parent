package com.chua.playwright.support;

import com.chua.playwright.support.bridge.PlaywrightNative;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * 批量命令构建器。一次 JNI 调用执行多条命令，显著减少跨语言往返开销。
 *
 * <pre>{@code
 * Batch b = new Batch();
 * int iLaunch = b.launch("chromium", true);
 * int iPage = b.newPage(iLaunch);
 * b.gotoPage(iPage, "https://example.com");
 * b.screenshot(iPage);
 * Batch.Result r = b.execute();
 * long realBrowserHandle = r.handle(iLaunch);
 * }</pre>
 */
public class Batch {

    private final List<Map<String, Object>> commands = new ArrayList<>();
    private final List<String> names = new ArrayList<>();
    private boolean stopOnError = true;

    public Batch stopOnError(boolean stop) {
        this.stopOnError = stop;
        return this;
    }

    public int size() {
        return commands.size();
    }

    public int launch(String browserType, boolean headless) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("browserType", browserType);
        p.put("headless", headless);
        return add("launch", null, p);
    }

    public int newContext(int browserHandle) {
        return add("newContext", browserHandle, null);
    }

    public int newPage(int targetHandle) {
        return add("newPage", targetHandle, null);
    }

    public void gotoPage(int pageHandle, String url) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("url", url);
        add("goto", pageHandle, p);
    }

    public void click(int handle, String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        add("click", handle, p);
    }

    public void fill(int handle, String selector, String value) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("value", value);
        add("fill", handle, p);
    }

    public void press(int handle, String selector, String key) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        p.put("key", key);
        add("press", handle, p);
    }

    public void setViewport(int handle, int w, int h) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("width", w);
        p.put("height", h);
        add("setViewportSize", handle, p);
    }

    public void evaluate(int handle, String expression) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("expression", expression);
        add("evaluate", handle, p);
    }

    public void screenshot(int handle) {
        add("screenshot", handle, null);
    }

    public void waitForSelector(int handle, String selector) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("selector", selector);
        add("waitForSelector", handle, p);
    }

    public void close(int handle) {
        add("close", handle, null);
    }

    public void raw(String action, Integer handle, Map<String, Object> params) {
        add(action, handle, params);
    }

    private int add(String action, Integer handle, Map<String, Object> params) {
        Map<String, Object> cmd = new LinkedHashMap<>();
        cmd.put("action", action);
        if (handle != null) {
            cmd.put("handle", handle);
        }
        if (params != null) {
            cmd.put("params", params);
        }
        commands.add(cmd);
        names.add(action);
        return commands.size() - 1;
    }

    public Result execute() {
        Map<String, Object> params = new LinkedHashMap<>();
        params.put("commands", commands);
        params.put("stopOnError", stopOnError);
        Map<String, Object> cmd = new LinkedHashMap<>();
        cmd.put("action", "batch");
        cmd.put("params", params);
        try {
            String resp = PlaywrightNative.execute(Playwright.OM.writeValueAsString(cmd));
            Map<String, Object> m = Playwright.OM.readValue(resp, Map.class);
            if (!Boolean.TRUE.equals(m.get("ok"))) {
                throw new PlaywrightException(String.valueOf(m.get("error")));
            }
            List<Object> data = (List<Object>) m.get("data");
            return new Result(names, data);
        } catch (PlaywrightException e) {
            throw e;
        } catch (Exception e) {
            throw new PlaywrightException("批量执行失败", e);
        }
    }

    /** 批量执行结果。 */
    public static class Result {
        private final List<String> names;
        private final List<Object> data;

        Result(List<String> names, List<Object> data) {
            this.names = names;
            this.data = data;
        }

        /** 获取第 index 条命令的真实句柄（如 launch/newPage 返回的 handle）。 */
        public long handle(int index) {
            Map<String, Object> d = (Map<String, Object>) data.get(index);
            return ((Number) d.get("handle")).longValue();
        }

        /** 获取第 index 条命令返回的字符串结果。 */
        public String string(int index) {
            Object o = data.get(index);
            return o == null ? null : String.valueOf(o);
        }

        /** 获取第 index 条命令返回的原始 JSON 对象。 */
        @SuppressWarnings("unchecked")
        public Map<String, Object> map(int index) {
            return (Map<String, Object>) data.get(index);
        }

        public int size() {
            return data.size();
        }
    }
}
