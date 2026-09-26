package com.chua.nativewechat.uia;

import com.chua.nativeuia.support.UiaSelector;
import lombok.Data;
import lombok.experimental.Accessors;

import java.util.ArrayList;
import java.util.List;

/**
 * 微信 PC 客户端（3.9.x）控件定位模板。
 *
 * <p>把"微信的会话列表在哪、消息列表在哪、输入框在哪"这类知识从代码里抽出来，
 * 变成可替换的配置对象。微信一旦升级导致控件层级变化，只需要替换本模板
 * （或从外部 JSON 加载），无需改动 {@link WechatUiaPollDirectory} 的逻辑。</p>
 *
 * <h3>调优方法</h3>
 * <p>控件层级不准时，先用 {@code UiaBridge.dumpTree(14, 3000)} 导出真实控件树，
 * 对照输出修改各选择器。微信 3.9.x 是原生 Qt 客户端（窗口类名形如
 * {@code Qt51514QWindowIcon}），UIA 可稳定读到 {@code List}/{@code ListItem}/{@code Edit}。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Data
@Accessors(chain = true)
public class WechatUiaSelectors {

    /**
     * 主窗口标题候选子串，按顺序尝试。
     *
     * <p>微信 3.9.x 的主聊天窗口标题随界面语言在「微信」与「Weixin」之间变化，
     * 且内置文章浏览器等附属窗口也常带「微信」标题，因此这里列出多个候选，
     * 由原生层在命中项中取面积最大的窗口。</p>
     */
    private List<String> windowTitles = new ArrayList<>(List.of("微信", "Weixin"));

    /**
     * 主窗口类名过滤。
     *
     * <p>微信 3.9.x 全部窗口类名形如 {@code Qt51514QWindowIcon}，随 Qt 版本号变化，
     * 因此只按前缀约束，避免版本升级后失效。传空表示不限制。</p>
     */
    private String windowClassPrefix = "Qt";

    /**
     * 是否要求主窗口可见（最小化状态下控件树不完整，读不到消息）
     */
    private boolean requireWindowVisible = true;

    /**
     * 左侧会话列表：会话项所在容器
     */
    private UiaSelector conversationList = UiaSelector.of("List")
            .setRequireEnabled(true)
            .setMaxDepth(6);

    /**
     * 左侧会话列表项：单个会话
     */
    private UiaSelector conversationItem = UiaSelector.of("ListItem")
            .setRequireEnabled(true)
            .setRequireOnscreen(true)
            .setMaxDepth(4);

    /**
     * 右侧消息列表：消息项所在容器
     */
    private UiaSelector messageList = UiaSelector.of("List")
            .setRequireEnabled(true)
            .setMaxDepth(8);

    /**
     * 右侧消息列表项：单条消息气泡
     */
    private UiaSelector messageItem = UiaSelector.of("ListItem")
            .setRequireEnabled(true)
            .setMaxDepth(6);

    /**
     * 底部输入框
     */
    private UiaSelector inputBox = UiaSelector.of("Edit")
            .setRequireEnabled(true)
            .setMaxDepth(8);

    /**
     * 聊天标题栏：用于发送前的会话校验，防止回错会话
     */
    private UiaSelector chatTitle = UiaSelector.of("Text")
            .setRequireEnabled(true)
            .setRequireOnscreen(true)
            .setMaxDepth(3);

    /**
     * "发送"按钮。取不到时回退为 {@code Enter} 提交
     */
    private UiaSelector sendButton = UiaSelector.of("Button")
            .setRequireEnabled(true)
            .setMaxDepth(8);

    /**
     * 消息文本所在的子控件：微信 3.9 的 ListItem 文本常挂在子 Text 上，
     * 取 ListItem 自身的 Name 只能拿到摘要
     */
    private UiaSelector messageText = UiaSelector.of("Text");

    /**
     * "我"发出的消息前缀。命中此前缀的消息视为自己发出，不触发回复
     */
    private List<String> selfPrefixes = new ArrayList<>(List.of("我:", "我："));

    /**
     * 是否跳过自己发出的消息。
     *
     * <p><b>必须保持 true</b>：轮询读到的是"对方发的 + 自己发的"全部消息，
     * 若不过滤，机器人会把自己的回复也当成新消息再回复，形成自问自答死循环。</p>
     *
     * <p>副作用：<b>发给自己（文件传输助手）的消息同样会被判为"我"发出并跳过</b>，
     * 因此该模式下机器人不会回复。详见 {@code WechatUiaPollDirectory#explainSelfChat()}。</p>
     */
    private boolean skipSelfMessages = true;

    /**
     * 是否跳过非文本消息（图片 / 文件 / 语音等）
     */
    private boolean skipNonText = true;

    /**
     * 非文本消息占位符，命中则跳过（不送 AI、不回复）
     */
    private List<String> nonTextMarkers = new ArrayList<>(List.of(
            "[图片]", "[文件]", "[视频]", "[语音]", "[表情]", "[动画表情]",
            "[链接]", "[位置]", "[名片]", "[转账]", "[红包]", "[引用]", "[合并转发]"));

    /**
     * 使用默认模板。
     *
     * @return 选择器模板
     */
    public static WechatUiaSelectors defaults() {
        return new WechatUiaSelectors();
    }
}
