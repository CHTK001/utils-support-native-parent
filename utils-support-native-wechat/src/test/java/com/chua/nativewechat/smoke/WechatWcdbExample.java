package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.ResultSet;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * 微信 WCDB 密钥自动提取与验证工具。
 *
 * <p>支持两种取钥方式：</p>
 * <ol>
 *   <li>显式传入 {@code --key}（64 位原始密钥或 96 位完整密钥）；</li>
 *   <li>传入 {@code --key-info-db}，从 key_info.db 中按候选偏移量批量
 *       提取候选密钥并逐一尝试，命中即输出可用密钥。</li>
 * </ol>
 * <p>64 位密钥会依据 session.db 文件头的 16 字节 salt 自动补齐为 96 位完整密钥。</p>
 *
 * <p>用法：</p>
 * <pre>
 * java --enable-native-access=ALL-UNNAMED --enable-preview \
 *      -cp target/classes:target/test-classes:$(cat target/cp.txt) \
 *      com.chua.nativewechat.smoke.WechatWcdbExample \
 *      --session-db=E:/微信/xwechat_files/wxid_xxx/db_storage/session/session.db \
 *      --key-info-db=E:/微信/xwechat_files/wxid_xxx/db_storage/key_info.db
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class WechatWcdbExample {

    /**
     * key_info.db 中候选密钥的字节偏移量集合
     */
    private static final int[] CANDIDATE_OFFSETS = {
            22, 38, 44, 54, 64, 74, 84, 94, 104, 114, 124, 134, 144, 154, 164, 174
    };

    /**
     * 命令行入口。
     *
     * @param args 命令行参数，见类注释用法
     * @throws Exception 参数非法或读取密钥库失败时抛出
     */
    public static void main(String[] args) throws Exception {
        System.exit(execute(args));
    }

    /**
     * 按参数取钥并验证。
     *
     * @param args 命令行参数
     * @return 存在可用密钥返回 0，全部候选失败返回 1
     * @throws Exception 参数非法或读取密钥库失败时抛出
     */
    private static int execute(String[] args) throws Exception {
        Arguments arguments = Arguments.parse(args);
        String sessionDb = arguments.required("session-db");
        String key = arguments.optional("key");
        String salt = readDbSalt(Path.of(sessionDb));
        if (key == null) {
            List<String> candidates = extractCandidates(Path.of(arguments.required("key-info-db")));
            for (String candidate : candidates) {
                if (tryKey(sessionDb, candidate + salt)) {
                    return 0;
                }
            }
            return 1;
        }
        if (key.length() == 64) {
            key += salt;
        }
        if (key.length() != 96) {
            throw new IllegalArgumentException("--key 必须是 64 位或 96 位十六进制密钥");
        }
        return tryKey(sessionDb, key) ? 0 : 1;
    }

    private static boolean tryKey(String sessionDb, String key) {
        try {
            WechatWcdbBridge bridge = WechatWcdbBridge.load();
            long handle = bridge.openAccount(sessionDb, key);
            try {
                String sessions = bridge.getSessions(handle);
                if (sessions == null || sessions.isBlank()) {
                    return false;
                }
                System.out.println("WCDB 验证成功，会话响应长度=" + sessions.length());
                return true;
            } finally {
                bridge.closeAccount(handle);
                bridge.close();
            }
        } catch (Exception e) {
            System.err.println("WCDB 验证失败：" + e.getMessage());
            return false;
        }
    }

    private static List<String> extractCandidates(Path keyInfoDb) throws Exception {
        List<String> result = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        try (Connection connection = DriverManager.getConnection("jdbc:sqlite:" + keyInfoDb);
             Statement statement = connection.createStatement();
             ResultSet resultSet = statement.executeQuery(
                     "SELECT key_info_data FROM LoginKeyInfoTable ORDER BY rowid DESC")) {
            while (resultSet.next()) {
                byte[] data = resultSet.getBytes(1);
                if (data == null || data.length < 35) {
                    continue;
                }
                byte[] inner = Arrays.copyOfRange(data, 3, data.length);
                for (int offset : CANDIDATE_OFFSETS) {
                    if (inner.length >= offset + 32) {
                        String candidate = toHex(Arrays.copyOfRange(inner, offset, offset + 32));
                        if (seen.add(candidate)) {
                            result.add(candidate);
                        }
                    }
                }
            }
        }
        return result;
    }

    private static String readDbSalt(Path databasePath) throws Exception {
        byte[] header = Files.readAllBytes(databasePath);
        if (header.length < 16) {
            throw new IllegalArgumentException("数据库文件头不足 16 字节：" + databasePath);
        }
        return toHex(Arrays.copyOfRange(header, 0, 16));
    }

    private static String toHex(byte[] bytes) {
        return java.util.HexFormat.of().formatHex(bytes);
    }

    private record Arguments(java.util.Map<String, String> values) {

        private static Arguments parse(String[] args) {
            java.util.Map<String, String> values = new java.util.HashMap<>();
            for (String argument : args) {
                if (!argument.startsWith("--") || !argument.contains("=")) {
                    throw new IllegalArgumentException("参数必须使用 --key=value 格式：" + argument);
                }
                String[] pair = argument.substring(2).split("=", 2);
                values.put(pair[0], pair[1]);
            }
            return new Arguments(values);
        }

        private String required(String key) {
            String value = values.get(key);
            if (value == null || value.isBlank()) {
                throw new IllegalArgumentException("缺少参数 --" + key);
            }
            return value;
        }

        private String optional(String key) {
            return values.get(key);
        }
    }
}
