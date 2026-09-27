package com.chua.nmap.support.bridge;

/**
 * nmap JNI 冒烟测试（自包含，不依赖项目任何模块）。
 *
 * <p>用法：{@code java RustNmapBridge <库路径>}</p>
 *
 * <p><b>类名必须与原生符号一致</b>：动态库导出的是
 * {@code Java_com_chua_nmap_support_bridge_RustNmapBridge_*}，把本类的全限定名
 * 编译进了二进制，故这里必须用与生产类相同的包名与类名。本文件只存在于
 * {@code src/smoke} 下，由 CI 单独编译，与生产类不会同时参与编译。</p>
 */
public final class RustNmapBridge {

    /**
     * 单端口 TCP 探测（仅声明，用于冒烟）。
     */
    private static native int scanSingleTcpPort(String host, int port, int timeout);

    /**
     * 服务探测（仅声明，用于冒烟）。
     */
    private static native String detectService(String host, int port, int timeout);

    /**
     * 入口。
     *
     * @param argv 第一个元素为动态库路径
     */
    public static void main(String[] argv) {
        if (argv.length == 0) {
            System.err.println("用法: RustNmapBridge <库路径>");
            System.exit(2);
        }
        System.load(argv[0]);
        log("已加载: " + argv[0]);

        // 1) 单端口探测应正常返回（0/1 均属正常，不应抛异常）
        int closed = scanSingleTcpPort("127.0.0.1", 1, 500);
        log("scanSingleTcpPort(127.0.0.1:1) = " + closed);
        require(closed == 0 || closed == 1, "单端口探测应返回 0 或 1，实际 " + closed);

        // 2) detectService 的 JSON 必须不含裸控制字符
        //    此前该函数手拼 JSON，banner 原文里的 CR/LF 未转义，调用方解析必失败
        String json = detectService("127.0.0.1", 80, 5000);
        log("detectService(127.0.0.1:80) = " + json);
        require(json != null && !json.isEmpty(), "detectService 返回为空");
        require(json.trim().startsWith("{") && json.trim().endsWith("}"), "返回值不是 JSON 对象");
        require(json.contains("\"port\":80"), "JSON 应包含 port:80");
        require(!json.contains("\r"), "JSON 内不得含裸 CR（转义后应为 \\r）");
        require(!json.contains("\n"), "JSON 内不得含裸 LF（转义后应为 \\n）");

        log("SMOKE OK");
    }

    /**
     * 断言，失败即抛错终止。
     *
     * @param condition 条件
     * @param message   失败描述
     */
    private static void require(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError("ASSERT FAIL " + message);
        }
        log("ASSERT ok   " + message);
    }

    /**
     * 输出日志。
     *
     * @param line 内容
     */
    private static void log(String line) {
        System.out.println(line);
    }
}
