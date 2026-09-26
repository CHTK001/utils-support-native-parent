package com.chua.nativewechat.uia;

import lombok.Data;
import lombok.experimental.Accessors;

/**
 * 微信消息事件：一条被监听到的新消息。
 *
 * <p>注意：UIA 只能读到界面呈现的文本，拿不到微信内部的消息 ID。
 * 因此 {@link #messageId()} 是"发送者 + 可见文本 + 观察时间"派生出的稳定去重键，
 * 而非微信官方 msgId。若业务需要真实 msgId，应改走 WCDB 直读路径
 * （{@code utils-support-native-wechat} 的 {@code WechatWcdbBridge}）。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Data
@Accessors(chain = true)
public class WechatUiaMessage {

    /**
     * 会话标题（私聊为对方昵称，群聊为群名）
     */
    private String conversationTitle;

    /**
     * 源用户显示名。群聊中为实际发言者，私聊中通常等于 {@link #conversationTitle}
     */
    private String sourceUser;

    /**
     * 消息正文。取自子 Text 控件，取不到时回退为消息项的 Name
     */
    private String content;

    /**
     * 消息项控件上的原始 Name（调试与回溯用）
     */
    private String rawName;

    /**
     * 消息项是否在当前视图内可见
     */
    private boolean onscreen = true;

    /**
     * 观察时间戳（毫秒）
     */
    private long observedAt = System.currentTimeMillis();

    /**
     * 是否群聊会话
     */
    private boolean groupChat;

    /**
     * 是否疑似自己发出的消息
     */
    private boolean selfMessage;

    /**
     * 是否为非文本消息（图片 / 文件 / 语音等）
     */
    private boolean nonText;

    /**
     * 生成去重键。
     *
     * <p>UIA 读不到官方 msgId，只能基于可见内容派生。文本完全相同的连续多条消息
     * 会被视为重复，这是本方案的固有精度上限，需要严格区分时应改走 WCDB。</p>
     *
     * @return 去重键
     */
    public String messageId() {
        return conversationTitle + '\0' + sourceUser + '\0' + content;
    }
}
