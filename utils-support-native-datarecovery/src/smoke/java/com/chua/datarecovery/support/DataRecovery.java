package com.chua.datarecovery.support;

import java.nio.file.Files;
import java.nio.file.Path;

/**
 * datarecovery JNI 冒烟测试（自包含，不依赖项目任何模块）。
 *
 * <p>用法：{@code java DataRecovery <库路径>}</p>
 *
 * <p><b>类名必须与原生符号一致</b>：动态库导出的是
 * {@code Java_com_chua_datarecovery_support_DataRecovery_*}，把本类的全限定名编译
 * 进了二进制，故这里必须用与生产类相同的包名与类名。本文件只存在于
 * {@code src/smoke} 下，由 CI 单独编译，与生产类不会同时参与编译。</p>
 */
public final class DataRecovery {

    /**
     * 扫描原生方法（仅声明，用于冒烟）。
     */
    private static native String nativeScan(String devicePath, int scanMode);

    /**
     * 入口。
     *
     * @param argv 第一个元素为动态库路径
     * @throws Exception 创建临时目录失败时
     */
    public static void main(String[] argv) throws Exception {
        if (argv.length == 0) {
            System.err.println("用法: DataRecovery <库路径>");
            System.exit(2);
        }
        System.load(argv[0]);
        log("已加载: " + argv[0]);

        // 1) 不存在的路径必须返回 success=false（此前原生硬编码为 true）
        Path missing = Path.of(System.getProperty("java.io.tmpdir"),
                "dr-smoke-missing-" + System.nanoTime());
        String j1 = nativeScan(missing.toString(), 0);
        log("missing -> " + j1);
        require(j1 != null, "不存在路径的返回为 null");
        require(j1.contains("\"success\":false"), "不存在路径应返回 success=false");
        require(j1.contains("does not exist"), "message 应点明路径不存在");

        // 2) 有效路径返回 success=true，且 message 带模式与计数
        Path tmp = Files.createTempDirectory("dr-smoke");
        Files.writeString(tmp.resolve("a.dat"), "x");
        String j2 = nativeScan(tmp.toString(), 0);
        log("valid   -> " + j2);
        require(j2 != null && j2.contains("\"success\":true"), "有效路径应返回 success=true");
        require(j2.contains("mode=walkdir"), "message 应包含 mode=walkdir");
        require(j2.contains("scanned="), "message 应包含 scanned 计数");

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
