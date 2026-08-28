package com.chua.playwright.support;

import com.chua.playwright.support.bridge.PlaywrightNative;
import com.fasterxml.jackson.databind.ObjectMapper;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * 对应 playwright-java 的 {@code Playwright} 入口。
 * 内部通过 JNI 将命令转发给 Rust 端（playwright-rs 实现）。
 */
public class Playwright {

    static final ObjectMapper OM = new ObjectMapper();

    static {
        PlaywrightNative.ensureLoaded();
    }

    private Playwright() {
    }

    public static Playwright create() {
        return new Playwright();
    }

    public static String version() {
        return PlaywrightNative.getVersion();
    }

    public BrowserType chromium() {
        return new BrowserType("chromium");
    }

    public APIRequestContext request() {
        long handle = requestHandle("newAPIRequest", null, null);
        return new APIRequestContext(handle);
    }

    // ===================== 命令转发辅助 =====================

    static Map<String, Object> request(String action, Long handle, Map<String, Object> params) {
        Map<String, Object> cmd = new LinkedHashMap<>();
        cmd.put("action", action);
        if (handle != null) {
            cmd.put("handle", handle);
        }
        if (params != null) {
            cmd.put("params", params);
        }
        try {
            String resp = PlaywrightNative.execute(OM.writeValueAsString(cmd));
            Map<String, Object> m = OM.readValue(resp, Map.class);
            if (!Boolean.TRUE.equals(m.get("ok"))) {
                throw new PlaywrightException(String.valueOf(m.get("error")));
            }
            Object data = m.get("data");
            if (data instanceof Map) {
                return (Map<String, Object>) data;
            }
            Map<String, Object> wrap = new LinkedHashMap<>();
            wrap.put("value", data);
            return wrap;
        } catch (PlaywrightException e) {
            throw e;
        } catch (Exception e) {
            throw new PlaywrightException("命令执行失败: " + action, e);
        }
    }

    static Object requestData(String action, Long handle, Map<String, Object> params) {
        Map<String, Object> cmd = new LinkedHashMap<>();
        cmd.put("action", action);
        if (handle != null) {
            cmd.put("handle", handle);
        }
        if (params != null) {
            cmd.put("params", params);
        }
        try {
            String resp = PlaywrightNative.execute(OM.writeValueAsString(cmd));
            Map<String, Object> m = OM.readValue(resp, Map.class);
            if (!Boolean.TRUE.equals(m.get("ok"))) {
                throw new PlaywrightException(String.valueOf(m.get("error")));
            }
            return m.get("data");
        } catch (PlaywrightException e) {
            throw e;
        } catch (Exception e) {
            throw new PlaywrightException("命令执行失败: " + action, e);
        }
    }

    static long requestHandle(String action, Long handle, Map<String, Object> params) {
        Map<String, Object> d = request(action, handle, params);
        return ((Number) d.get("handle")).longValue();
    }

    static String asString(Object o) {
        return o == null ? null : String.valueOf(o);
    }

    static List<Long> asHandleList(Map<String, Object> d) {
        return (List<Long>) d.get("handles");
    }
}
