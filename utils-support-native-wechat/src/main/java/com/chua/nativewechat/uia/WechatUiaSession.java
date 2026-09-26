package com.chua.nativewechat.uia;

import lombok.Getter;
import lombok.extern.slf4j.Slf4j;

import java.time.Duration;
import java.util.List;
import java.util.Objects;

/**
 * 微信会话：一次轮询中某个会话内的新消息集合，并支持回信。
 *
 * <p>实例由 {@link WechatUiaPollDirectory#poll()} 创建，与之共享底层 UIA 桥接器，
 * 因此必须使用 try-with-resources 或在用完后调用 {@link #close()}，
 * 否则 COM 上下文与元素池不会释放。</p>
 *
 * <h3>回信为何需要"校验"</h3>
 * <p>UI 自动化发消息的唯一路径是"切到目标会话 → 往输入框写文本 → 提交"，
 * 而切会话依赖界面上的<b>昵称文本</b>。昵称可能重复、可能与群名撞车、
 * 可能刚被对方改动，一旦切错就会把私聊内容发进错误的会话。
 * 因此 {@link #reply(String)} 在写入前强制校验标题栏等于本会话标题，
 * 校验不过宁可不发——这是本类最重要的安全约束，不可关闭。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
@Getter
public final class WechatUiaSession implements AutoCloseable {

    /**
     * 会话标题（私聊为对方昵称，群聊为群名）
     */
    private final String title;

    /**
     * 本次轮询发现的源用户集合，保持出现顺序
     */
    private final List<String> sourceUsers;

    /**
     * 本次轮询发现的新消息
     */
    private final List<WechatUiaMessage> messages;

    /**
     * 是否群聊会话
     */
    private final boolean groupChat;

    /**
     * 本次轮询最后一条消息的观察时间
     */
    private final long lastMessageAt;

    /**
     * 轮询目录，所有定位与输入操作都经由它下发
     */
    private final WechatUiaPollDirectory directory;

    /**
     * 是否已关闭，防止重复释放底层资源
     */
    private boolean closed;

    /**
     * 构造会话。
     *
     * @param title        会话标题
     * @param sourceUsers  源用户集合
     * @param messages     新消息列表
     * @param groupChat    是否群聊
     * @param lastMessageAt 最后消息观察时间
     * @param directory    轮询目录
     */
    WechatUiaSession(String title, List<String> sourceUsers, List<WechatUiaMessage> messages,
                     boolean groupChat, long lastMessageAt, WechatUiaPollDirectory directory) {
        this.title = title;
        this.sourceUsers = List.copyOf(sourceUsers);
        this.messages = List.copyOf(messages);
        this.groupChat = groupChat;
        this.lastMessageAt = lastMessageAt;
        this.directory = directory;
    }

    /**
     * 取合并后的消息正文。
     *
     * <p>人发消息是分段的（"帮我查下" / "明天的会议室"），直接逐条送去 LLM 会得到半截回答。
     * 这里按出现顺序拼接，供上层一次性提交。</p>
     *
     * @param separator 连接符
     * @return 合并后的文本
     */
    public String mergedContent(String separator) {
        StringBuilder sb = new StringBuilder();
        for (WechatUiaMessage m : messages) {
            if (sb.length() > 0) {
                sb.append(separator);
            }
            sb.append(m.getContent());
        }
        return sb.toString();
    }

    /**
     * 取距离上一条消息的时长。
     *
     * @return 时长；无消息时返回 {@link Duration#ZERO}
     */
    public Duration age() {
        if (messages.isEmpty()) {
            return Duration.ZERO;
        }
        return Duration.ofMillis(System.currentTimeMillis() - lastMessageAt);
    }

    /**
     * 向本会话回信。
     *
     * <p>执行链路：切到目标会话 → 校验标题 → 写输入框 → 提交。三个写入策略按可靠性降级：
     * {@code Value} 模式（不抢焦点，最优）→ 剪贴板 + {@code Ctrl+V} → 发送按钮 / {@code Enter}。</p>
     *
     * <p><b>注意：</b>回信会抢占微信窗口焦点，调用方需自行控制与用户正常使用的并发关系。</p>
     *
     * @param text 回复正文
     * @return 发送结果
     */
    public WechatUiaReplyResult reply(String text) {
        Objects.requireNonNull(directory, "会话未绑定轮询目录");
        if (closed) {
            throw new IllegalStateException("会话已关闭，无法回信: " + title);
        }
        return directory.reply(title, text);
    }

    @Override
    public void close() {
        closed = true;
    }

    @Override
    public String toString() {
        return "WechatUiaSession{title='" + title + "', group=" + groupChat
                + ", messages=" + messages.size() + "}";
    }
}
