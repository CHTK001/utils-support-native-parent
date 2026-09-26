package com.chua.nativewechat.uia;

import com.chua.nativeuia.support.UiaBridge;
import com.chua.nativeuia.support.UiaElementInfo;
import com.chua.nativeuia.support.UiaSelector;
import lombok.extern.slf4j.Slf4j;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/**
 * 微信会话轮询目录：轮询式监听微信 PC 客户端（3.9.x）的新消息，按会话聚合，并支持回信。
 *
 * <h3>为什么用轮询而不是事件订阅</h3>
 * <p>UIA 提供 {@code IUIAutomation} 事件（属性变更、焦点、树结构变更），
 * 但事件源需要目标客户端注册相应的 UIA 代理，Qt 客户端支持不完整且极易漏事件。
 * 轮询虽然有延迟，但行为确定、失败可观测，因此作为默认方案。</p>
 *
 * <h3>工作原理</h3>
 * <ol>
 *   <li>遍历左侧会话列表项，取每个会话的标题（会话名）与未读数；</li>
 *   <li>对"有新消息迹象"的会话逐个打开，读取右侧消息列表；</li>
 *   <li>用去重键过滤掉已处理的消息，聚合成 {@link WechatUiaSession} 返回；</li>
 *   <li>回信时切回目标会话，<b>校验标题一致后</b>才写入并提交。</li>
 * </ol>
 *
 * <h3>关键约束</h3>
 * <ul>
 *   <li>微信窗口必须<b>可见且未最小化</b>：最小化时 Qt 侧不实例化内部控件，
 *       UIA 只能看到 3~4 个节点，读不到任何消息；</li>
 *   <li>实例<b>不是线程安全</b>，且 COM 单元与线程绑定，必须在同一线程内创建和使用；</li>
 *   <li>{@link #reply(String)} 会抢占窗口焦点，调用方需自行处理与用户正常使用的并发。</li>
 * </ul>
 *
 * <h3>使用示例</h3>
 * <pre>{@code
 * try (WechatUiaPollDirectory dir = WechatUiaPollDirectory.open()) {
 *     dir.markBaseline();                       // 冷启动：不回溯历史
 *     while (running) {
 *         try (WechatUiaPollDirectory.PollBatch batch = dir.poll()) {
 *             for (WechatUiaSession session : batch.sessions()) {
 *                 String question = session.mergedContent(" ");
 *                 String answer = llm.ask(question);
 *                 WechatUiaReplyResult r = session.reply(answer);
 *                 if (!r.isSuccess()) {
 *                     log.warn("回信失败: {}", r.getError());
 *                 }
 *             }
 *         }
 *         Thread.sleep(2000);
 *     }
 * }
 * }</pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class WechatUiaPollDirectory implements AutoCloseable {

    /**
     * 单次轮询每个会话最多读取的消息条数。
     * UIA 遍历成本与节点数线性相关，微信消息列表通常只渲染可见的十几条，
     * 因此这个值主要影响兜底场景，不宜调大。
     */
    private static final int MAX_MESSAGES_PER_SESSION = 60;

    /**
     * 默认单条回复长度上限，避免超长文本撑爆输入框
     */
    private static final int DEFAULT_MAX_REPLY_LEN = 500;

    /**
     * 写入文本后等待界面提交的时长（毫秒）
     */
    private static final long SUBMIT_SETTLE_MILLIS = 150L;

    /**
     * 切换会话后等待界面刷新的时长（毫秒）
     */
    private static final long SWITCH_SETTLE_MILLIS = 350L;

    /**
     * 底层 UIA 桥接器
     */
    private final UiaBridge bridge;

    /**
     * 控件定位模板
     */
    private final WechatUiaSelectors selectors;

    /**
     * 单条回复长度上限
     */
    private final int maxReplyLength;

    /**
     * 已处理消息的去重键
     */
    private final Set<String> seenKeys = new LinkedHashSet<>();

    /**
     * 绑定的窗口句柄
     */
    private long windowHandle;

    /**
     * 是否已关闭
     */
    private boolean closed;

    /**
     * 会话标题到最近一次消息观察时间的映射，用于排序与"是否有新消息"判断
     */
    private final java.util.Map<String, Long> lastSeenAt = new java.util.HashMap<>();

    /**
     * 私有构造器。
     *
     * @param bridge        UIA 桥接器
     * @param selectors     控件定位模板
     * @param maxReplyLength 单条回复长度上限
     */
    private WechatUiaPollDirectory(UiaBridge bridge, WechatUiaSelectors selectors,
                                    int maxReplyLength) {
        this.bridge = bridge;
        this.selectors = selectors;
        this.maxReplyLength = maxReplyLength;
    }

    /**
     * 打开轮询目录并绑定微信窗口。
     *
     * @return 轮询目录
     * @throws IllegalStateException 微信未运行或窗口未找到时抛出
     */
    public static WechatUiaPollDirectory open() {
        return open(WechatUiaSelectors.defaults());
    }

    /**
     * 以指定控件定位模板打开轮询目录。
     *
     * @param selectors 控件定位模板
     * @return 轮询目录
     * @throws IllegalStateException 微信未运行或窗口未找到时抛出
     */
    public static WechatUiaPollDirectory open(WechatUiaSelectors selectors) {
        UiaBridge bridge = UiaBridge.load();
        try {
            WechatUiaPollDirectory dir = new WechatUiaPollDirectory(bridge, selectors,
                    DEFAULT_MAX_REPLY_LEN);
            dir.windowHandle = attachFirst(bridge, selectors.getWindowTitles(),
                    selectors.getWindowClassPrefix(), selectors.isRequireWindowVisible());
            log.info("微信 UIA 轮询目录已就绪, hwnd={:#x}", dir.windowHandle);
            return dir;
        } catch (RuntimeException | Error e) {
            bridge.close();
            throw e;
        }
    }

    /**
     * 按标题候选列表依次尝试绑定，返回第一个成功的窗口句柄。
     *
     * <p>微信 3.9.x 的主聊天窗口标题随界面语言在「微信」与「Weixin」之间变化，
     * 因此按候选顺序逐个尝试；全部失败时抛异常并附带最后一次错误。</p>
     *
     * @param bridge        UIA 桥接器
     * @param titles        标题候选子串列表
     * @param classPrefix   窗口类名前缀，null 或空表示不限制
     * @param requireVisible 是否要求窗口可见
     * @return 绑定到的窗口句柄
     */
    private static long attachFirst(UiaBridge bridge, List<String> titles,
                                    String classPrefix, boolean requireVisible) {
        RuntimeException last = null;
        for (String title : titles) {
            try {
                return bridge.attachWindow(title, classPrefix, requireVisible);
            } catch (IllegalStateException e) {
                last = e;
                log.debug("标题 [{}] 未命中: {}", title, e.getMessage());
            }
        }
        throw new IllegalStateException("未找到微信主窗口，已尝试标题: " + titles
                + "。请确认微信已登录且主窗口可见（最小化时控件树不完整）。", last);
    }

    /**
     * 取底层桥接器，供上层做诊断（如导出控件树）。
     *
     * @return UIA 桥接器
     */
    public UiaBridge bridge() {
        return bridge;
    }

    /**
     * 取绑定的窗口句柄。
     *
     * @return 窗口句柄
     */
    public long windowHandle() {
        return windowHandle;
    }

    /**
     * 判断微信窗口是否仍然有效。
     *
     * @return 有效返回 true
     */
    public boolean isAlive() {
        return !closed && bridge.isWindowAlive(windowHandle);
    }

    /**
     * 冷启动标记：把当前所有会话的可见消息记为已读，**不触发回复**。
     *
     * <p>不调用本方法会导致进程启动后把历史消息全部当作新消息回复一遍。</p>
     *
     * @return 标记的消息条数
     */
    public int markBaseline() {
        int marked = 0;
        for (String title : listConversationTitles()) {
            if (openConversation(title)) {
                for (WechatUiaMessage m : readCurrentMessages()) {
                    seenKeys.add(m.messageId());
                    marked++;
                }
            }
        }
        log.info("冷启动基线已标记 {} 条历史消息为已读", marked);
        return marked;
    }

    /**
     * 执行一次轮询，返回本轮新消息聚合出的会话。
     *
     * <p>调用方必须关闭返回的 {@link PollBatch}，否则元素池不会释放。</p>
     *
     * @return 轮询批次
     */
    public PollBatch poll() {
        if (closed) {
            throw new IllegalStateException("轮询目录已关闭");
        }
        if (!isAlive()) {
            throw new IllegalStateException("微信窗口已失效，请重新 open()");
        }
        List<WechatUiaSession> sessions = new ArrayList<>(4);
        for (String title : listConversationTitles()) {
            try {
                if (!openConversation(title)) {
                    log.debug("跳过无法打开的会话: {}", title);
                    continue;
                }
                List<WechatUiaMessage> fresh = new ArrayList<>(4);
                for (WechatUiaMessage m : readCurrentMessages()) {
                    // 先做业务过滤再入去重集：被跳过的消息不应占用去重槽位，
                    // 否则调整过滤规则后重跑会因"已见过"而永久漏掉
                    if (selectors.isSkipSelfMessages() && m.isSelfMessage()) {
                        log.debug("跳过自己发出的消息 [{}]: {}", title, m.getContent());
                        continue;
                    }
                    if (selectors.isSkipNonText() && m.isNonText()) {
                        log.debug("跳过非文本消息 [{}]: {}", title, m.getRawName());
                        continue;
                    }
                    if (seenKeys.add(m.messageId())) {
                        fresh.add(m);
                    }
                }
                if (fresh.isEmpty()) {
                    lastSeenAt.put(title, System.currentTimeMillis());
                    continue;
                }
                List<String> users = new ArrayList<>(2);
                for (WechatUiaMessage m : fresh) {
                    if (!users.contains(m.getSourceUser())) {
                        users.add(m.getSourceUser());
                    }
                }
                long lastAt = fresh.get(fresh.size() - 1).getObservedAt();
                lastSeenAt.put(title, lastAt);
                sessions.add(new WechatUiaSession(title, users, fresh, isGroupTitle(title),
                        lastAt, this));
            } catch (RuntimeException e) {
                log.warn("轮询会话 [{}] 失败: {}", title, e.getMessage());
            }
        }
        return new PollBatch(sessions);
    }

    /**
     * 向指定会话回信。
     *
     * <p>内部会切到目标会话并<b>强制校验标题栏</b>；校验不通过则拒绝发送，
     * 避免把内容发进昵称相近的其他会话。</p>
     *
     * @param title 目标会话标题
     * @param text  回复正文
     * @return 发送结果
     */
    public WechatUiaReplyResult reply(String title, String text) {
        if (closed) {
            return WechatUiaReplyResult.fail(title, "轮询目录已关闭");
        }
        if (text == null || text.isBlank()) {
            return WechatUiaReplyResult.fail(title, "回复内容为空");
        }
        String payload = text.replaceAll("[\\r\\n]+", " ").trim();
        if (payload.length() > maxReplyLength) {
            payload = payload.substring(0, maxReplyLength);
        }

        try {
            if (!openConversation(title)) {
                return WechatUiaReplyResult.fail(title, "无法切换到目标会话");
            }
            // 发送前校验：标题必须仍然匹配，防止切会话失败或切错后误发
            String actualTitle = readChatTitle();
            if (actualTitle == null || actualTitle.isBlank()) {
                return WechatUiaReplyResult.fail(title, "读不到标题栏，无法确认目标会话，拒绝发送");
            }
            if (!titlesMatch(actualTitle, title)) {
                return WechatUiaReplyResult.fail(title,
                        "标题校验失败：期望 [" + title + "] 实际 [" + actualTitle + "]，拒绝发送");
            }

            long inputId = bridge.findOne(selectors.getInputBox().toJson());
            if (inputId == 0L) {
                return WechatUiaReplyResult.fail(title, "找不到输入框（微信窗口是否最小化？）");
            }
            // 输入框句柄必须在提交完成前保持有效，因此释放放在 finally
            try {
                String via;
                if (bridge.setValue(inputId, payload)) {
                    via = "ValuePattern";
                } else {
                    // Value 模式不可用时回退剪贴板，中文输入不要用 SendInput 逐字模拟
                    if (!bridge.setClipboard(payload)) {
                        return WechatUiaReplyResult.fail(title, "写入剪贴板失败");
                    }
                    bridge.setFocus(inputId);
                    if (!bridge.sendKeys("{CTRL}v")) {
                        return WechatUiaReplyResult.fail(title,
                                "Ctrl+V 下发失败（目标窗口未取得前台焦点）");
                    }
                    via = "Clipboard+Ctrl+V";
                }
                settle(SUBMIT_SETTLE_MILLIS);

                if (!submit()) {
                    return WechatUiaReplyResult.fail(title, "提交失败（输入框有内容但未能发出）");
                }
                log.info("已回信 [{}] {} 字, via={}", title, payload.length(), via);
                return WechatUiaReplyResult.ok(title, payload, via);
            } finally {
                bridge.release(inputId);
            }
        } catch (RuntimeException e) {
            log.warn("回信 [{}] 异常: {}", title, e.getMessage());
            return WechatUiaReplyResult.fail(title, e.getMessage());
        }
    }

    /**
     * 提交输入框内容：优先点发送按钮，缺失时用 Enter。
     *
     * @return 提交成功返回 true
     */
    private boolean submit() {
        long sendId = bridge.findOne(selectors.getSendButton().toJson());
        if (sendId != 0L) {
            try {
                if (bridge.invoke(sendId)) {
                    return true;
                }
            } finally {
                bridge.release(sendId);
            }
        }
        // 微信默认回车发送；此时窗口已是前台
        return bridge.sendKeys("{ENTER}");
    }

    /**
     * 列出左侧会话列表的全部标题。
     *
     * @return 会话标题列表，保持界面顺序
     */
    public List<String> listConversationTitles() {
        List<String> titles = new ArrayList<>(16);
        long[] ids = bridge.find(selectors.getConversationList().toJson(), 1);
        if (ids.length == 0) {
            log.warn("找不到会话列表（微信窗口是否最小化？）");
            return titles;
        }
        long listId = ids[0];
        try {
            // 选择器本身限定在整棵树内，这里再按"矩形落在列表容器内"收敛一次，
            // 避免把消息列表里的项误当成会话项
            long[] itemIds = filterChildrenOf(listId,
                    bridge.find(selectors.getConversationItem().toJson(), 200));
            for (UiaElementInfo item : bridge.describeInfos(itemIds)) {
                String title = cleanConversationTitle(item.getName());
                if (title != null && !titles.contains(title)) {
                    titles.add(title);
                }
            }
        } finally {
            bridge.release(listId);
        }
        return titles;
    }

    /**
     * 读取当前会话的消息列表。
     *
     * @return 消息列表，按界面顺序
     */
    public List<WechatUiaMessage> readCurrentMessages() {
        List<WechatUiaMessage> out = new ArrayList<>(MAX_MESSAGES_PER_SESSION);
        long[] ids = bridge.find(selectors.getMessageItem().toJson(), MAX_MESSAGES_PER_SESSION);
        if (ids.length == 0) {
            return out;
        }
        String title = readChatTitle();
        boolean group = isGroupTitle(title);
        for (UiaElementInfo item : bridge.describeInfos(ids)) {
            WechatUiaMessage msg = toMessage(item, title, group);
            if (msg != null) {
                out.add(msg);
            }
        }
        bridge.releaseAll();
        return out;
    }

    /**
     * 读取标题栏会话名。
     *
     * @return 会话名；读不到返回 null
     */
    public String readChatTitle() {
        long id = bridge.findOne(selectors.getChatTitle().toJson());
        if (id == 0L) {
            return null;
        }
        try {
            String name = bridge.getProperty(id, "name");
            return name == null ? null : name.trim();
        } finally {
            bridge.release(id);
        }
    }

    /**
     * 切换到指定会话。
     *
     * @param title 会话标题
     * @return 切换成功返回 true
     */
    public boolean openConversation(String title) {
        long[] ids = bridge.find(selectors.getConversationItem().toJson(), 200);
        long[] matched = filterByTitle(ids, title);
        if (matched.length == 0) {
            log.debug("会话列表中未找到: {}", title);
            return false;
        }
        long id = matched[0];
        try {
            boolean ok = bridge.invoke(id) || bridge.click(id);
            if (ok) {
                settle(SWITCH_SETTLE_MILLIS);
            }
            return ok;
        } catch (RuntimeException e) {
            log.debug("切换会话 [{}] 异常: {}", title, e.getMessage());
            return false;
        } finally {
            bridge.release(id);
        }
    }

    /**
     * 判断会话标题是否代表群聊。
     *
     * <p>UIA 读不到可靠的群标识，这里按业界通行做法用群人数后缀启发式判断。
     * 拿不准时按私聊处理（更保守，不会误判成群而在群里发言）。</p>
     *
     * @param title 会话标题
     * @return 疑似群聊返回 true
     */
    public boolean isGroupTitle(String title) {
        if (title == null) {
            return false;
        }
        return title.matches(".*[(（]\\s*\\d{2,}\\s*人\\s*\\)）].*");
    }

    /**
     * 导出当前窗口的控件树 JSON，用于选择器调优。
     *
     * @param maxDepth 最大深度
     * @param maxNodes 最大节点数
     * @return 控件树 JSON
     */
    public String dumpTree(int maxDepth, int maxNodes) {
        return bridge.dumpTree(maxDepth, maxNodes);
    }

    /**
     * 等待界面刷新。
     *
     * <p>捕获 {@link InterruptedException} 而不向上抛，避免受检异常污染上层 API；
     * 同时恢复中断标志，保证上层仍能正确响应取消。</p>
     *
     * @param millis 等待毫秒数
     */
    private static void settle(long millis) {
        try {
            Thread.sleep(millis);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    @Override
    public void close() {
        if (closed) {
            return;
        }
        closed = true;
        try {
            bridge.releaseAll();
        } catch (RuntimeException e) {
            log.debug("释放元素池异常: {}", e.getMessage());
        }
        bridge.close();
        log.info("微信 UIA 轮询目录已关闭");
    }

    // ------------------------------------------------------------------ 内部工具

    /**
     * 把控件树上的消息项转成消息事件。
     *
     * @param item  消息项快照
     * @param title 当前会话标题
     * @param group 是否群聊
     * @return 消息事件；无法解析时返回 null
     */
    private WechatUiaMessage toMessage(UiaElementInfo item, String title, boolean group) {
        String raw = item.getName();
        if (raw == null || raw.isBlank()) {
            return null;
        }
        raw = raw.trim();
        String text = item.getValue();
        if (text == null || text.isBlank() || text.equals(raw)) {
            // 消息文本常挂在子 Text 上，Value 与 Name 相同时回退用 Name
            text = raw;
        }

        String sender = "";
        String body = text;
        int idx = indexOfSeparator(text);
        if (idx > 0) {
            String head = text.substring(0, idx).trim();
            if (isPlausibleSender(head)) {
                sender = head;
                body = text.substring(idx + 1).trim();
            }
        }
        if (sender.isEmpty()) {
            sender = group ? "" : title;
        }

        String flat = body.toLowerCase(Locale.ROOT);
        boolean nonText = selectors.getNonTextMarkers().stream().anyMatch(flat::contains);
        boolean self = sender.startsWith("我")
                || selectors.getSelfPrefixes().stream().anyMatch(body::startsWith);

        return new WechatUiaMessage()
                .setConversationTitle(title)
                .setSourceUser(sender)
                .setContent(body)
                .setRawName(raw)
                .setOnscreen(!Boolean.TRUE.equals(item.getOffscreen()))
                .setGroupChat(group)
                .setSelfMessage(self)
                .setNonText(nonText)
                .setObservedAt(System.currentTimeMillis());
    }

    /**
     * 查找"发送者:内容"中的分隔符位置。
     *
     * @param text 原始文本
     * @return 分隔符下标；不存在返回 -1
     */
    private int indexOfSeparator(String text) {
        int c = text.indexOf(':');
        int f = text.indexOf('：');
        if (c < 0) {
            return f;
        }
        if (f < 0) {
            return c;
        }
        return Math.min(c, f);
    }

    /**
     * 判断前缀是否像发送者昵称（长度合理且不含空格）。
     *
     * @param head 分隔符之前的文本
     * @return 形似昵称返回 true
     */
    private boolean isPlausibleSender(String head) {
        return !head.isEmpty() && head.length() <= 32 && !head.contains(" ");
    }

    /**
     * 清洗会话标题：剥离未读数角标与静音标记。
     *
     * @param raw 原始名称
     * @return 清洗后的标题；无效返回 null
     */
    private String cleanConversationTitle(String raw) {
        if (raw == null) {
            return null;
        }
        String t = raw.trim();
        t = t.replaceAll("^[（(]\\d+[)）]", "");
        t = t.replaceAll("[（(]\\d+[)）]$", "");
        t = t.replaceAll("^\\d+", "");
        t = t.replaceAll("[\\[\\]【】]", "").trim();
        return t.isEmpty() ? null : t;
    }

    /**
     * 比较两个会话标题是否指向同一会话。
     *
     * @param a 标题 A
     * @param b 标题 B
     * @return 视为同一会话返回 true
     */
    private boolean titlesMatch(String a, String b) {
        String ca = cleanConversationTitle(a);
        String cb = cleanConversationTitle(b);
        return ca != null && cb != null && ca.equals(cb);
    }

    /**
     * 在元素句柄数组中筛选标题匹配的元素。
     *
     * @param ids   元素句柄数组
     * @param title 目标标题
     * @return 匹配到的句柄（至多 1 个）
     */
    private long[] filterByTitle(long[] ids, String title) {
        if (ids.length == 0) {
            return ids;
        }
        for (UiaElementInfo info : bridge.describeInfos(ids)) {
            if (titlesMatch(info.getName(), title)) {
                return new long[]{info.getId()};
            }
        }
        return new long[0];
    }

    /**
     * 筛选出矩形落在父容器范围内的元素。
     *
     * <p>UIA 没有"按容器查询子元素"的直接接口，通过矩形包含关系近似收敛，
     * 可避免把消息列表里的项误当成会话项。</p>
     *
     * @param parentId 父元素句柄
     * @param ids      候选元素句柄
     * @return 落在父范围内的元素句柄
     */
    private long[] filterChildrenOf(long parentId, long[] ids) {
        if (ids.length == 0) {
            return ids;
        }
        int[] parentRect = bridge.getRect(parentId);
        if (parentRect[2] <= parentRect[0] || parentRect[3] <= parentRect[1]) {
            // 父容器矩形无效（窗口最小化等），不做过滤
            return ids;
        }
        List<UiaElementInfo> all = bridge.describeInfos(ids);
        List<Long> keep = new ArrayList<>(all.size());
        for (UiaElementInfo info : all) {
            if (info.getRect() == null || !info.getRect().isValid()) {
                keep.add(info.getId());
                continue;
            }
            int l = info.getRect().getLeft();
            int t = info.getRect().getTop();
            int r = info.getRect().getRight();
            int b = info.getRect().getBottom();
            if (l >= parentRect[0] && t >= parentRect[1]
                    && r <= parentRect[2] && b <= parentRect[3]) {
                keep.add(info.getId());
            }
        }
        long[] result = new long[keep.size()];
        for (int i = 0; i < keep.size(); i++) {
            result[i] = keep.get(i);
        }
        return result;
    }

    /**
     * 取元素快照列表的首个非空元素。
     *
     * @param list 元素快照列表
     * @return 首个元素；列表为空返回 null
     */
    private UiaElementInfo firstOf(List<UiaElementInfo> list) {
        return list == null || list.isEmpty() ? null : list.get(0);
    }

    /**
     * 单次轮询的批次结果。
     *
     * <p>持有期间会占用原生元素池中的句柄，必须关闭。</p>
     */
    /**
     * 判断某个会话是否为"自聊"（发给自己，如文件传输助手），并给出解释。
     *
     * <p><b>自聊无法被本方案回复</b>：UIA 只能看到界面上的发送者显示名，
     * 而自己发给自己的消息显示名同样是本账号昵称。因此"对方发的"与"自己发的"
     * 在自聊场景下<b>文本层面完全无法区分</b>——而这个区分恰恰是防死循环的必需条件。
     * 结果是：自聊里发出的任何消息都会被判为"自己发的"而跳过。</p>
     *
     * <p>可行的验证方式是<b>小号与主号对聊</b>：消息来自另一个账号，
     * 显示名可区分，回复链路完整可用。</p>
     *
     * @param title 会话标题
     * @return 自聊诊断结论
     */
    public WechatUiaSelfChatDiagnosis explainSelfChat(String title) {
        if (!openConversation(title)) {
            return WechatUiaSelfChatDiagnosis.unreadable(title,
                    "无法切换到该会话，窗口可能已最小化");
        }
        List<WechatUiaMessage> messages = readCurrentMessages();
        if (messages.isEmpty()) {
            return WechatUiaSelfChatDiagnosis.unreadable(title, "会话内没有可见消息，无法判定");
        }
        long selfCount = messages.stream().filter(WechatUiaMessage::isSelfMessage).count();
        boolean allSelf = selfCount == messages.size();
        String note = allSelf
                ? "该会话消息全部显示为本账号发出（自聊），skipSelfMessages=true 时本方案不会回复。"
                + "请改用小号与主号对聊进行验证。"
                : "该会话存在非本账号发出的消息，回信链路可用。";
        return new WechatUiaSelfChatDiagnosis(title, true, allSelf, messages.size(),
                (int) selfCount, note);
    }

    /**
     * 列出全部会话的自聊诊断结论。
     *
     * @return 每个会话一条诊断
     */
    public List<WechatUiaSelfChatDiagnosis> diagnoseAllConversations() {
        List<WechatUiaSelfChatDiagnosis> out = new ArrayList<>(16);
        for (String title : listConversationTitles()) {
            try {
                out.add(explainSelfChat(title));
            } catch (RuntimeException e) {
                log.debug("诊断会话 [{}] 失败: {}", title, e.getMessage());
            }
        }
        return out;
    }

    public static final class PollBatch implements AutoCloseable {

        /**
         * 本轮发现的会话
         */
        private final List<WechatUiaSession> sessions;

        /**
         * 构造批次。
         *
         * @param sessions 会话列表
         */
        PollBatch(List<WechatUiaSession> sessions) {
            this.sessions = List.copyOf(sessions);
        }

        /**
         * 取本轮发现的会话。
         *
         * @return 会话列表，不可变
         */
        public List<WechatUiaSession> sessions() {
            return sessions;
        }

        /**
         * 合并本轮所有会话的消息正文。
         *
         * @return 合并文本
         */
        public String allContent() {
            StringBuilder sb = new StringBuilder();
            for (WechatUiaSession s : sessions) {
                if (sb.length() > 0) {
                    sb.append('\n');
                }
                sb.append(s.getTitle()).append(": ").append(s.mergedContent(" "));
            }
            return sb.toString();
        }


    @Override
        public void close() {
            sessions.forEach(WechatUiaSession::close);
        }
    }
}
