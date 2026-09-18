package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

/**
 * 最小候选 key 验证器（SQLCipher 4.x binary mode：传 64 位 raw key，自动读 salt）。
 *
 * @author CH
 * @since 4.0.0.42
 */
public class KeyVerify {

    /**
     * 程序入口，运行示例自检。
     *
     * @param args 参数，不允许为 null
     * @throws Exception 当执行过程不满足前置条件时
     */
    public static void main(String[] args) throws Exception {
        String sessionDb = "C:\\Users\\Administrator\\Documents\\WXWork\\1688850006200900\\Data\\session.db";
        if (args.length >= 1) {
            sessionDb = args[0];
        }

        String[] candidates;
        if (args.length >= 2) {
            candidates = args[1].split(",");
        } else {
            candidates = new String[]{
                    "b3b521bffaf709bf1a161ae73ef25d92",
            };
        }

        System.out.println("session.db: " + sessionDb);
        System.out.println("候选数: " + candidates.length);
        System.out.println();

        WechatWcdbBridge bridge = WechatWcdbBridge.load();
        int success = 0;

        for (int i = 0; i < candidates.length; i++) {
            String rawKey = candidates[i].trim();
            /* 如果超过 64 位，取前 64 位（normalize_key 行为） */
            if (rawKey.length() > 64) {
                rawKey = rawKey.substring(0, 64);
            }
            System.out.println("--- 候选 " + (i + 1) + " raw=" + rawKey);
            try {
                long handle = bridge.openAccount(sessionDb, rawKey);
                System.out.println("  打开成功, handle=0x" + Long.toHexString(handle));

                String sessions = bridge.getSessions(handle);
                System.out.println("  会话 JSON 长度: " + (sessions != null ? sessions.length() : "null"));
                if (sessions != null && sessions.length() > 2) {
                    System.out.println("  前 500 字符: " + sessions.substring(0, Math.min(500, sessions.length())));
                }

                if (sessions != null && sessions.contains("username")) {
                    int idx = sessions.indexOf("\"username\"");
                    if (idx > 0) {
                        String tail = sessions.substring(idx + 11);
                        int end = tail.indexOf("\"");
                        if (end > 0) {
                            String firstUsername = tail.substring(0, end);
                            System.out.println("  第一个会话: " + firstUsername);
                            try {
                                int count = bridge.getMessageCount(handle, firstUsername);
                                System.out.println("  消息数: " + count);
                                if (count > 0) {
                                    String msgs = bridge.getMessages(handle, firstUsername, 5, 0);
                                    System.out.println("  前 5 条消息: " + msgs);
                                }
                            } catch (Exception e) {
                                System.out.println("  消息查询失败: " + e.getMessage());
                            }
                        }
                    }
                }

                bridge.closeAccount(handle);
                success++;
                System.out.println("  >>> 成功! <<<");
                break;
            } catch (Exception e) {
                System.out.println("  失败: " + e.getMessage());
            }
        }

        if (success == 0) {
            System.out.println("\n所有候选均失败。");
        } else {
            System.out.println("\n验证通过！");
        }
        bridge.close();
    }
}
