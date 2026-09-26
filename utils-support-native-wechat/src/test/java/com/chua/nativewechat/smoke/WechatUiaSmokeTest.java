package com.chua.nativewechat.smoke;

import com.chua.nativeuia.support.UiaBridge;
import com.chua.nativeuia.support.UiaElementInfo;
import com.chua.nativeuia.support.UiaSelector;
import com.chua.nativewechat.uia.WechatUiaMessage;
import com.chua.nativewechat.uia.WechatUiaPollDirectory;
import com.chua.nativewechat.uia.WechatUiaReplyResult;
import com.chua.nativewechat.uia.WechatUiaSession;
import com.chua.nativewechat.uia.WechatUiaSelectors;
import lombok.extern.slf4j.Slf4j;

import java.util.List;

/**
 * 微信 3.9.x UIA 会话轮询端到端冒烟测试。
 *
 * <p>按阶段逐步验证，逐段可读：
 * 动态库抽取与符号绑定 → COM 上下文 → 窗口绑定 → 控件树导出 → 选择器查找 →
 * 会话列举 → 冷启动基线 → 轮询 → 回信。</p>
 *
 * <p><b>前置条件</b>：PC 微信已登录且窗口可见。窗口最小化时 Qt 侧不实例化内部控件，
 * UIA 只能看到 3~4 个节点，本测试会在阶段 4 明确报出这一情况而非静默通过。</p>
 *
 * <p><b>注意</b>：阶段 8 会真实发送一条消息，请只在测试号与自己的号之间验证。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class WechatUiaSmokeTest {

    /**
     * 阶段计数
     */
    private static int stage = 0;

    private WechatUiaSmokeTest() {
    }

    /**
     * 入口。
     *
     * @param args 忽略
     */
    public static void main(String[] args) {
        boolean doReply = args.length > 0 && "--reply".equals(args[0]);
        try {
            if (!UiaBridge.isSupported()) {
                fail("当前平台不是 Windows，UIA 不可用");
            }
            stage(1, "动态库抽取与符号绑定");
            try (UiaBridge bridge = UiaBridge.load()) {
                if (!bridge.isCreated()) {
                    fail("COM 上下文创建失败");
                }
                stage(2, "COM 上下文", "handle=" + bridge.context());

                stage(3, "绑定微信窗口");
                long hwnd;
                try {
                    hwnd = bridge.attachWindow("微信", true);
                } catch (IllegalStateException e) {
                    fail("未找到微信窗口: " + e.getMessage());
                    return;
                }
                out("  窗口句柄 hwnd=0x" + Long.toHexString(hwnd));

                stage(4, "导出控件树");
                String tree = bridge.dumpTree(14, 3000);
                int nodeCount = countNodes(tree);
                out("  控件树节点数=" + nodeCount + " 字节数=" + tree.length());
                if (nodeCount < 20) {
                    out("  !! 节点过少，微信窗口很可能已最小化。"
                            + "请恢复窗口后重跑，否则后续阶段读不到任何消息。");
                }

                stage(5, "选择器查找 List / ListItem / Edit");
                countByType(bridge, "List");
                countByType(bridge, "ListItem");
                countByType(bridge, "Edit");
                countByType(bridge, "Text");
                countByType(bridge, "Button");
            }

                stage(6, "打开轮询目录并列举会话");
            try (WechatUiaPollDirectory dir = WechatUiaPollDirectory.open()) {
                List<String> titles = dir.listConversationTitles();
                out("  会话数=" + titles.size());
                for (int i = 0; i < Math.min(titles.size(), 20); i++) {
                    out("    [" + i + "] " + titles.get(i));
                }
                if (titles.isEmpty()) {
                    out("  !! 没读到会话，窗口最小化时必然如此。");
                }

                stage(7, "冷启动基线 + 轮询");
                int marked = dir.markBaseline();
                out("  基线标记 " + marked + " 条");
                try (WechatUiaPollDirectory.PollBatch batch = dir.poll()) {
                    out("  本轮新会话数=" + batch.sessions().size());
                    for (WechatUiaSession s : batch.sessions()) {
                        out("    会话 [" + s.getTitle() + "] 消息 " + s.getMessages().size() + " 条");
                        for (WechatUiaMessage m : s.getMessages()) {
                            out("      <" + m.getSourceUser() + "> " + m.getContent()
                                    + " self=" + m.isSelfMessage() + " nonText=" + m.isNonText());
                        }
                    }
                }

                stage(8, "回信" + (doReply ? "" : "（未启用 --reply，跳过真实发送）"));
                if (!doReply || titles.isEmpty()) {
                    out("  跳过。加 --reply 参数并确保有会话时才会真实发送。");
                } else {
                    String target = titles.get(0);
                    out("  目标会话: " + target);
                    WechatUiaReplyResult r = dir.reply(target, "【冒烟测试】这条消息来自 UIA 自动化自测");
                    out("  结果 success=" + r.isSuccess()
                            + " via=" + r.getSubmitVia()
                            + " error=" + r.getError());
                    if (!r.isSuccess()) {
                        fail("回信失败: " + r.getError());
                    }
                }
            }
            out("");
            out("全部阶段通过");
        } catch (Throwable t) {
            fail("阶段 " + stage + " 异常: " + t);
            t.printStackTrace();
        }
    }

    /**
     * 打印阶段标题。
     *
     * @param no    阶段号
     * @param title 阶段名
     */
    private static void stage(int no, String title) {
        stage = no;
        out("");
        out("=== 阶段 " + no + ": " + title + " ===");
    }

    /**
     * 打印阶段标题与结果。
     *
     * @param no     阶段号
     * @param title  阶段名
     * @param result 结果描述
     */
    private static void stage(int no, String title, String result) {
        stage(no, title);
        out("  " + result);
    }

    /**
     * 按控件类型统计命中数量。
     *
     * @param bridge      UIA 桥接器
     * @param controlType 控件类型名
     */
    private static void countByType(UiaBridge bridge, String controlType) {
        try {
            long[] ids = bridge.find(UiaSelector.of(controlType).setMaxDepth(14).toJson(), 500);
            out("  " + controlType + " 命中 " + ids.length + " 个");
            bridge.releaseAll();
            if (ids.length > 0) {
                List<UiaElementInfo> infos = bridge.describeInfos(ids);
                for (int i = 0; i < Math.min(infos.size(), 8); i++) {
                    UiaElementInfo info = infos.get(i);
                    out("    " + controlType + " name=" + quote(info.getName())
                            + " value=" + quote(shorten(info.getValue()))
                            + " patterns=" + info.getPatterns()
                            + " offscreen=" + info.getOffscreen());
                }
            }
        } catch (RuntimeException e) {
            out("  " + controlType + " 查询失败: " + e.getMessage());
        }
    }

    /**
     * 统计 JSON 中出现的节点数（粗略按字段名计数）。
     *
     * @param json 控件树 JSON
     * @return 节点数
     */
    private static int countNodes(String json) {
        int n = 0;
        int i = 0;
        while ((i = json.indexOf("\"controlType\"", i)) >= 0) {
            n++;
            i += 13;
        }
        return n;
    }

    /**
     * 截断过长文本。
     *
     * @param s 原文
     * @return 截断后的文本
     */
    private static String shorten(String s) {
        if (s == null) {
            return null;
        }
        return s.length() <= 60 ? s : s.substring(0, 60) + "...";
    }

    /**
     * 带引号输出，避免空白字符造成误读。
     *
     * @param s 原文
     * @return 带引号的文本
     */
    private static String quote(String s) {
        return s == null ? "null" : "\"" + s.replace("\n", "\\n") + "\"";
    }

    /**
     * 输出一行。
     *
     * @param text 文本
     */
    private static void out(String text) {
        System.out.println(text);
    }

    /**
     * 报告失败并结束进程。
     *
     * @param message 失败原因
     */
    private static void fail(String message) {
        System.out.println("[FAIL] " + message);
        System.exit(1);
    }
}
