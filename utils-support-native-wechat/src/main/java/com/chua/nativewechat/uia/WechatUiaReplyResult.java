package com.chua.nativewechat.uia;

import lombok.Builder;
import lombok.Data;

/**
 * 微信回信结果。
 *
 * @author CH
 * @since 4.0.0.42
 */
@Data
@Builder
public class WechatUiaReplyResult {

    /**
     * 目标会话标题
     */
    private String conversationTitle;

    /**
     * 是否发送成功
     */
    private boolean success;

    /**
     * 实际写入的文本（可能被长度上限截断）
     */
    private String sentText;

    /**
     * 提交方式，用于排查"文本写进去了但没发出去"
     */
    private String submitVia;

    /**
     * 失败原因；成功时为 null
     */
    private String error;

    /**
     * 构造成功结果。
     *
     * @param title     会话标题
     * @param sentText  实际写入文本
     * @param submitVia 提交方式
     * @return 结果
     */
    public static WechatUiaReplyResult ok(String title, String sentText, String submitVia) {
        return WechatUiaReplyResult.builder()
                .conversationTitle(title)
                .success(true)
                .sentText(sentText)
                .submitVia(submitVia)
                .build();
    }

    /**
     * 构造失败结果。
     *
     * @param title 会话标题
     * @param error 失败原因
     * @return 结果
     */
    public static WechatUiaReplyResult fail(String title, String error) {
        return WechatUiaReplyResult.builder()
                .conversationTitle(title)
                .success(false)
                .error(error)
                .build();
    }
}
