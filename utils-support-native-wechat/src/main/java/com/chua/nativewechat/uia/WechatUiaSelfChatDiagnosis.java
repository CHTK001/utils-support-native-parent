package com.chua.nativewechat.uia;

/**
 * 自聊诊断结论。
 *
 * <p>用于回答"能不能用发消息给自己（文件传输助手）的方式验证回信链路"这个问题。
 * 结论是<b>不能</b>，原因见 {@link WechatUiaPollDirectory#explainSelfChat(String)}。</p>
 *
 * @param title        会话标题
 * @param readable     是否成功读到该会话的消息
 * @param selfChat     是否为自聊（全部消息都显示为本账号发出）
 * @param totalCount   会话内可见消息总数
 * @param selfCount    其中显示为本账号发出的条数
 * @param note         结论说明
 * @author CH
 * @since 4.0.0.42
 */
public record WechatUiaSelfChatDiagnosis(
        String title,
        boolean readable,
        boolean selfChat,
        int totalCount,
        int selfCount,
        String note) {

    /**
     * 构造"无法读取"的诊断结果。
     *
     * @param title 会话标题
     * @param note  失败原因
     * @return 诊断结果
     */
    public static WechatUiaSelfChatDiagnosis unreadable(String title, String note) {
        return new WechatUiaSelfChatDiagnosis(title, false, false, 0, 0, note);
    }

    /**
     * 取非本账号发出的消息条数。
     *
     * @return 条数
     */
    public int inboundCount() {
        return Math.max(0, totalCount - selfCount);
    }
}
