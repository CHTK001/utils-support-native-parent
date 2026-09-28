package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;

/**
 * 微信 WCDB 端到端管道验证工具。
 *
 * <p>以真实的 session.db 与解密密钥走一遍
 * {@link WechatWcdbBridge} 的完整链路：加载动态库 → 打开并解密账号库 →
 * 读取会话列表 → 释放资源。用于确认动态库抽取、SQLCipher 解密与
 * 跨分片 ATTACH 在本机可用。</p>
 *
 * <p>用法：</p>
 * <pre>
 * java --enable-native-access=ALL-UNNAMED --enable-preview \
 *      -cp target/classes:target/test-classes:$(cat target/cp.txt) \
 *      com.chua.nativewechat.smoke.PipelineExample \
 *      --db=E:/微信/xwechat_files/wxid_xxx/db_storage/session/session.db \
 *      --key=64位hex原始密钥
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class PipelineExample {

    /**
     * 命令行入口。
     *
     * @param args 形如 {@code --db=<session.db 路径> --key=<hex 密钥>} 的参数
     * @throws Exception 参数非法或动态库/解密失败时抛出
     */
    public static void main(String[] args) throws Exception {
        System.exit(execute(args));
    }

    /**
     * 执行管道验证。
     *
     * @param args 命令行参数
     * @return 会话列表非空返回 0，否则返回 1
     * @throws Exception 打开或读取失败时抛出
     */
    private static int execute(String[] args) throws Exception {
        String database = required(args, "db");
        String key = required(args, "key");
        Path databasePath = Path.of(database);
        if (!Files.isRegularFile(databasePath)) {
            throw new IllegalArgumentException("数据库文件不存在：" + databasePath);
        }
        WechatWcdbBridge bridge = WechatWcdbBridge.load();
        long handle = bridge.openAccount(database, key);
        try {
            String sessions = bridge.getSessions(handle);
            if (sessions == null || sessions.isBlank()) {
                return 1;
            }
            System.out.println("WCDB 管道验证成功，会话响应长度=" + sessions.length());
            return 0;
        } finally {
            bridge.closeAccount(handle);
            bridge.close();
        }
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
