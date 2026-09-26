package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

import java.util.Arrays;

/**
 * 微信数据库候选密钥批量验证工具。
 *
 * <p>依次尝试若干候选密钥，成功打开账号库并读出非空会话列表即判定该密钥有效。
 * 适用于从多处获得的候选密钥中筛选出真正能解密当前 session.db 的那一个。</p>
 *
 * <p>用法：</p>
 * <pre>
 * java --enable-native-access=ALL-UNNAMED --enable-preview \
 *      -cp target/classes:target/test-classes:$(cat target/cp.txt) \
 *      com.chua.nativewechat.smoke.KeyExample \
 *      --db=E:/微信/xwechat_files/wxid_xxx/db_storage/session/session.db \
 *      --keys=候选密钥1,候选密钥2,候选密钥3
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class KeyExample {

    /**
     * 命令行入口。
     *
     * @param args 形如 {@code --db=<路径> --keys=<逗号分隔候选密钥>} 的参数
     * @throws Exception 参数非法时抛出
     */
    public static void main(String[] args) throws Exception {
        System.exit(execute(args));
    }

    /**
     * 依次尝试候选密钥。
     *
     * @param args 命令行参数
     * @return 存在可用密钥返回 0，全部失败返回 1
     * @throws Exception 参数非法时抛出
     */
    private static int execute(String[] args) throws Exception {
        String database = required(args, "db");
        String[] keys = required(args, "keys").split(",");
        for (String key : keys) {
            String candidate = key.trim();
            if (candidate.length() > 64) {
                candidate = candidate.substring(0, 64);
            }
            if (tryKey(database, candidate)) {
                return 0;
            }
        }
        return 1;
    }

    private static boolean tryKey(String database, String key) {
        try {
            WechatWcdbBridge bridge = WechatWcdbBridge.load();
            long handle = bridge.openAccount(database, key);
            try {
                String sessions = bridge.getSessions(handle);
                if (sessions != null && !sessions.isBlank()) {
                    System.out.println("候选密钥验证成功，响应长度=" + sessions.length());
                    return true;
                }
            } finally {
                bridge.closeAccount(handle);
                bridge.close();
            }
        } catch (Exception e) {
            System.err.println("候选密钥验证失败：" + e.getMessage());
        }
        return false;
    }

    private static String required(String[] args, String name) {
        String prefix = "--" + name + "=";
        return Arrays.stream(args)
                .filter(argument -> argument.startsWith(prefix))
                .map(argument -> argument.substring(prefix.length()))
                .filter(value -> !value.isBlank())
                .findFirst()
                .orElseThrow(() -> new IllegalArgumentException("缺少参数 --" + name));
    }
}
