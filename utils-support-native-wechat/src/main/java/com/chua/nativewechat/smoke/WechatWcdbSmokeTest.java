package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

import java.io.RandomAccessFile;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.sql.*;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * 微信 4.x WCDB 端到端冒烟测试。
 *
 * <p>从 key_info.db 提取候选 64 位 raw key，自动从 session.db 文件头读取 16 字节
 * salt 并拼接为 96 位完整密钥，通过 WechatWcdbBridge 打开 session.db 验证解密。</p>
 *
 * <p>支持参数：</p>
 * <ul>
 *   <li>无参数 — 自动遍历 key_info.db 全部候选密钥</li>
 *   <li>64 位 raw key hex — Rust 侧自动拼接 salt</li>
 *   <li>96 位完整密钥 hex（raw key + salt）— 直接传入</li>
 * </ul>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class WechatWcdbSmokeTest {

    /**
     * 冒烟测试入口。
     *
     * @param args 可选参数：
     * <ul>
     *   <li>无参数 — 自动遍历 key_info.db 全部候选密钥</li>
     *   <li>key_hex — 64 位 raw key（自动拼接 salt）或 96 位完整密钥</li>
     * </ul>
     */
    public static void main(String[] args) throws Exception {
        String keyInfoDb = "E:\\微信\\xwechat_files\\all_users\\login\\wxid_ag9726boqgse21\\key_info.db";
        String sessionDb = "E:\\微信\\xwechat_files\\wxid_ag9726boqgse21_b942\\db_storage\\session\\session.db";

        // 直接指定密钥（64 位或 96 位）
        if (args.length >= 1 && (args[0].length() == 64 || args[0].length() == 96)) {
            String salt = readDbSalt(sessionDb);
            System.out.println("session.db salt: " + salt);
            if (args[0].length() == 64) {
                System.out.println("拼接完整密钥: " + args[0] + salt);
                testOneKey(sessionDb, args[0] + salt);
            } else {
                testOneKey(sessionDb, args[0]);
            }
            return;
        }

        // 从 key_info.db 提取候选
        String salt = readDbSalt(sessionDb);
        System.out.println("=== 从 key_info.db 提取候选密钥 ===");
        System.out.println("session.db salt（文件头 16 字节）: " + salt);
        System.out.println();

        List<String> candidates = extractCandidates(keyInfoDb);
        System.out.println("共 " + candidates.size() + " 个候选 64 位 raw key，逐个验证（拼接 salt 后形成 96 位完整密钥）...\n");

        int success = 0;
        for (int i = 0; i < candidates.size(); i++) {
            String rawKey = candidates.get(i);
            String fullKey = rawKey + salt;
            System.out.println("--- 候选 " + (i + 1) + " / " + candidates.size() + "  raw=" + rawKey + "  full=" + fullKey);
            try {
                testOneKey(sessionDb, fullKey);
                System.out.println(">>> 成功! <<<");
                success++;
                break;
            } catch (Exception e) {
                System.out.println("  失败: " + e.getMessage());
            }
        }

        if (success == 0) {
            System.out.println("\n所有候选均失败。");
            System.out.println("提示: 也可手动指定 64 位 raw key 或 96 位完整密钥作为参数直接运行。");
        }
    }

    /**
     * 候选密钥在 key_info_data 内的偏移量（32 字节窗口）。
     */
    private static final int[] OFFSETS = {22, 38, 44, 54, 64, 74, 84, 94, 104, 114, 124, 134, 144, 154, 164, 174};

    /**
     * 从 key_info.db 读取全部 key_info_data，按不同偏移提取 32 字节候选 raw key。
     *
     * @param keyInfoDb key_info.db 路径
     * @return 候选 64 位 raw key hex 列表（去重）
     */
    private static List<String> extractCandidates(String keyInfoDb) throws Exception {
        List<String> result = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        try (Connection conn = DriverManager.getConnection("jdbc:sqlite:" + keyInfoDb)) {
            try (Statement st = conn.createStatement()) {
                ResultSet rs = st.executeQuery("SELECT key_info_data FROM LoginKeyInfoTable ORDER BY rowid DESC");
                while (rs.next()) {
                    byte[] data = rs.getBytes(1);
                    if (data == null || data.length < 32) {
                        continue;
                    }
                    // 外层 protobuf: 0a <len> [inner]
                    byte[] inner = java.util.Arrays.copyOfRange(data, 3, data.length);
                    for (int off : OFFSETS) {
                        if (inner.length >= off + 32) {
                            byte[] block = java.util.Arrays.copyOfRange(inner, off, off + 32);
                            String hex = toHex(block);
                            if (seen.add(hex)) {
                                result.add(hex);
                            }
                        }
                    }
                    // 最后一行的 last32
                    byte[] last32 = java.util.Arrays.copyOfRange(inner, inner.length - 32, inner.length);
                    String lastHex = toHex(last32);
                    if (seen.add(lastHex)) {
                        result.add(lastHex);
                    }
                }
            }
        }
        return result;
    }

    /**
     * 用指定密钥（64 位 raw key 或 96 位完整密钥）尝试打开 session.db 并查询。
     *
     * <p>Rust 侧 {@code open_sqlcipher} 会自动判断：
     * 64 位则从 DB 文件头读取 salt 拼接，96 位则直接使用。</p>
     *
     * @param sessionDb session.db 路径
     * @param keyHex    64 位或 96 位十六进制密钥
     */
    private static void testOneKey(String sessionDb, String keyHex) throws Exception {
        System.out.println("打开: " + sessionDb);
        System.out.println("密钥（" + keyHex.length() + " 位 hex）: " + keyHex);

        WechatWcdbBridge bridge = WechatWcdbBridge.load();
        long handle = bridge.openAccount(sessionDb, keyHex);
        System.out.println("打开成功, handle=0x" + Long.toHexString(handle));

        try {
            // 会话列表
            String sessionsJson = bridge.getSessions(handle);
            System.out.println("会话数: " + (sessionsJson != null ? sessionsJson.length() : "null") + " 字节");
            if (sessionsJson != null && sessionsJson.length() > 2) {
                System.out.println("前 200 字符: " + sessionsJson.substring(0, Math.min(200, sessionsJson.length())));
            }

            // 获取第一个会话的消息数
            if (sessionsJson != null && sessionsJson.contains("username")) {
                int idx = sessionsJson.indexOf("\"username\"");
                if (idx > 0) {
                    String tail = sessionsJson.substring(idx + 11);
                    int end = tail.indexOf("\"");
                    if (end > 0) {
                        String firstUsername = tail.substring(0, end);
                        int count = bridge.getMessageCount(handle, firstUsername);
                        System.out.println("会话 " + firstUsername + " 消息数: " + count);
                    }
                }
            }

            // 显示名
            try {
                String namesJson = bridge.getDisplayNames(handle, "[\"" + "wxid_ag9726boqgse21\" ]");
                System.out.println("显示名: " + namesJson);
            } catch (Exception e) {
                System.out.println("显示名查询失败（可选）: " + e.getMessage());
            }
        } finally {
            bridge.closeAccount(handle);
            bridge.close();
        }
        System.out.println("验证通过！\n");
    }

    /**
     * 读取 SQLite/SQLCipher 数据库文件头前 16 字节作为 salt（hex 字符串）。
     *
     * @param dbPath 数据库文件路径
     * @return 32 位 hex 字符串（16 字节 salt）
     */
    private static String readDbSalt(String dbPath) {
        try {
            byte[] header = Files.readAllBytes(Paths.get(dbPath));
            byte[] salt = java.util.Arrays.copyOfRange(header, 0, 16);
            return toHex(salt);
        } catch (Exception e) {
            System.err.println("读取 salt 失败（" + dbPath + "）: " + e.getMessage());
            return "00000000000000000000000000000000";
        }
    }

    /**
     * 字节数组转十六进制字符串。
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
