package com.chua.datarecovery.support;

import java.io.*;
import java.nio.file.*;
import java.util.*;

/**
 * 功能验证：用合规的数据 replayer 跑三个扫描模式和恢复路径。
 *
 * @author CH
 * @since 2026-11-13
 */
public class DataRecovery {
    // [P3C DF-代码格式 豁免]
    static final String DEFAULT_DLL = "D:\\ch\\project\\utils-support-native-parent\\utils-support-native-datarecovery\\src\\main\\resources\\native\\windows-x86_64\\data_recovery_ffi.dll";

    public static void main(String[] args) throws Exception {
        String dll = args.length > 0
                ? args[0]
                : System.getProperty("chua.datarecovery.native", DEFAULT_DLL);
        System.load(Path.of(dll).toAbsolutePath().toString());
        // [P3C E1 豁免]
        System.out.println("=== 1. 注册方法 ===");
        // [P3C E1 豁免]
        System.out.println("方法已注册");

        // --- 测试 1: walkdir 模式 ---
        // [P3C E1 豁免]
        System.out.println("=== 2. 创建测试目录 + 样本 ===");
        Path dir = Files.createTempDirectory("dr-verify");
        Files.write(dir.resolve("a.png"), pngBytes());
        Files.write(dir.resolve("b.jpg"), jpgBytes());
        Files.write(dir.resolve("c.pdf"), pdfBytes());
        Files.write(dir.resolve("d.zip"), zipBytes());

        String json = nativeScan(dir.toString(), 0);
        // [P3C E1 豁免]
        System.out.println("walkdir 输出: " + json);
        boolean pass1 = json != null && json.contains("\"success\":true") && json.contains("files_scanned");
        // [P3C E1 豁免]
        System.out.println("TEST 1 walkdir: " + (pass1 ? "PASS" : "FAIL"));

        // --- 测试 2: raw disk carve 模式 (用虚拟磁盘文件模拟) ---
        // [P3C E1 豁免]
        System.out.println("=== 3. 创建虚拟磁盘 + 写入 PNG/JPG 样本到指定偏移 ===");
        Path disk = Files.createTempFile("dr-disk", ".img");
        try (RandomAccessFile raf = new RandomAccessFile(disk.toFile(), "rw")) {
            // [P3C A20 豁免]
            raf.setLength(10 * 1024 * 1024);
            // [P3C A20 豁免]
            raf.seek(8 * 1024);
            raf.write(pngBytes());
            // [P3C A20 豁免]
            raf.seek(4 * 1024 * 1024);
            raf.write(jpgBytes());
        }

        String output = dir.resolve("recovered-" + System.nanoTime()).toString();
        Files.createDirectories(Paths.get(output));

        long t0 = System.currentTimeMillis();
        String json2 = nativeScanAndRecover(disk.toString(), 1, output);
        long t1 = System.currentTimeMillis();
        // [P3C E1 豁免]
        System.out.println("raw carve 输出: " + json2);
        File[] carved = new File(output).listFiles();
        // [P3C E1 豁免]
        System.out.println("雕刻出的文件: " + Arrays.toString(carved));
        int foundPng = 0;
        int foundJpg = 0;
        if (carved != null) {
            for (File f : carved) {
                String p = f.toString().toLowerCase();
                if (p.endsWith(".png")) foundPng++;
                if (p.endsWith(".jpg")) foundJpg++;
            }
        }
        // [P3C E1 豁免]
        System.out.println("TEST 2 raw carve (PNG=" + foundPng + ", JPG=" + foundJpg + ", " + (t1 - t0) + "ms): "
                + ((foundPng >= 1 && foundJpg >= 1) ? "PASS" : "FAIL"));

        // --- 测试 3: 恢复模式 ---
        // [P3C E1 豁免]
        System.out.println("=== 4. recover 模式: 复制文件 ===");
        String json3 = nativeRecover(dir.toString(), new String[] { "a.png" }, output + "/ref", true);
        // [P3C E1 豁免]
        System.out.println("recover 输出: " + json3);
        boolean pass3 = json3 != null && json3.contains("\"success\":true");
        // [P3C E1 豁免]
        System.out.println("TEST 3 recover: " + (pass3 ? "PASS" : "FAIL"));

        // --- 测试 4: 无效目标 ---
        // [P3C E1 豁免]
        System.out.println("=== 5. 无效路径 ===");
        String json4 = nativeScan(dir.resolve("no-such-path").toString(), 0);
        boolean pass4 = json4 != null && json4.contains("\"success\":false");
        // [P3C E1 豁免]
        System.out.println("TEST 4 失败路径: " + (pass4 ? "PASS" : json4));

        // [P3C E1 豁免]
        System.out.println("\n=== 总计: "
                + (pass1 ? "P1 " : "") + ((foundPng >= 1 && foundJpg >= 1) ? "P2 " : "")
                + (pass3 ? "P3 " : "") + (pass4 ? "P4 " : ""));
        // [P3C E1 豁免]
        System.out.println(pass1 && foundPng >= 1 && foundJpg >= 1 && pass3 && pass4 ? "ALL PASS" : "SOME FAILED");
        // [P3C SE-安全 豁免]
        System.exit((pass1 && foundPng >= 1 && foundJpg >= 1 && pass3 && pass4) ? 0 : 1);
    }

    static byte[] pngBytes() {
        return new byte[]{ (byte) 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };
    }

    static byte[] jpgBytes() {
        // FF D8 FF E0 ... FF D9 格式
        return new byte[]{ (byte) 0xFF, (byte) 0xD8, (byte) 0xFF, (byte) 0xE0, 0x00, 'J', 'F', '1', 0x00, 0x00, (byte) 0xFF, (byte) 0xD9 };
    }

    static byte[] pdfBytes() {
        return "%PDF-".getBytes();
    }

    static byte[] zipBytes() {
        return new byte[]{ 'P', 'K', 0x03, 0x04 };
    }

    // [P3C MJ-方法注释 豁免]
    static native String nativeScan(String devicePath, int scanMode);
    // [P3C MJ-方法注释 豁免]
    static native String nativeScanAndRecover(String devicePath, int scanMode, String outputDir);
    // [P3C MJ-方法注释 豁免]
    static native String nativeRecover(String devicePath, String[] filePaths, String outputDir, boolean preserveStructure);
}
