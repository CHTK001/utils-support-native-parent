package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Arrays;

/**
 * 解密管道端到端验证：自建 SQLCipher 数据库（已知 64 位 hex 密钥），
 * 通过 WechatWcdbBridge（Rust 原生库）读取并输出真实数据。
 *
 * <p>用于证明 Rust SQLCipher 解密管道本身正确（不依赖微信真实密钥）。</p>
 *
 * <p>使用方法：
 * 1. 运行 create_test_db.py 生成 target/test-sqlcipher/session.db
 * 2. 运行本类，自动读取并输出会话数据
 * </p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class PipelineVerify {

    /** 已知 64 位 hex 原始密钥（32 字节），与 create_test_db.py 中使用的 raw key 一致 */
    private static final String RAW_KEY =
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6" +
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";

    private static final String DB_DIR = "target\\test-sqlcipher";

    /**
     * 程序入口，运行示例自检。
     *
     * @param args 参数，不允许为 null
     * @throws Exception 当执行过程不满足前置条件时
     */
    public static void main(String[] args) throws Exception {
        Path dbPath = Paths.get(DB_DIR, "session.db");
        if (!Files.isRegularFile(dbPath)) {
            System.out.println("未找到 " + dbPath.toAbsolutePath());
            System.out.println("请先用 sqlcipher CLI 创建测试库（命令见类 Javadoc）。");
            return;
        }

        byte[] header = Files.readAllBytes(dbPath);
        String salt = toHex(Arrays.copyOfRange(header, 0, 16));
        System.out.println("=== 管道验证 ===");
        System.out.println("DB: " + dbPath.toAbsolutePath());
        System.out.println("salt: " + salt);
        System.out.println("raw key(64位, Rust 侧 build_full_key 取前 64 位): " + RAW_KEY);
        System.out.println();

        WechatWcdbBridge bridge = WechatWcdbBridge.load();
        // 传 64 位 raw key（SQLCipher x'...' 直接接受 32B raw key，salt 由库自动处理）
        long handle = bridge.openAccount(dbPath.toString(), RAW_KEY);
        System.out.println("打开成功 handle=0x" + Long.toHexString(handle));

        try {
            String sessionsJson = bridge.getSessions(handle);
            System.out.println("会话 JSON:");
            System.out.println(sessionsJson);

            int count = bridge.getMessageCount(handle, "wxid_test_001");
            System.out.println("wxid_test_001 消息数: " + count);
        } finally {
            bridge.closeAccount(handle);
            bridge.close();
        }
        System.out.println();
        System.out.println(">>> 管道验证通过 <<<");
    }

    /**
     * 转为Hex。
     *
     * @param bytes 字节数组，不允许为 null
     * @return 结果字符串
     */
    private static String toHex(byte[] bytes) {
        StringBuilder sb = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }
}
