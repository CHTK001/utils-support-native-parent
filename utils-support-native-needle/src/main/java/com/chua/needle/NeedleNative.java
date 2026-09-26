package com.chua.needle;

/**
 * Needle 推理引擎对外门面。
 *
 * <p>底层为 cactus-compute 官方 C 引擎的 FFM 绑定，实现见 {@link NeedleEngine}。
 * 本类只做可用性判断与异常翻译，不持有任何原生状态。</p>
 *
 * <p><b>部署要求（两项都必须提供，缺一不可）：</b></p>
 * <ol>
 *   <li>引擎动态库 {@code libneedle3.dll/.so/.dylib}，从 HuggingFace 获取，
 *       经 {@code -Dchua.needle.native.dir} 或 {@code CHUA_NEEDLE_NATIVE_DIR} 指定目录</li>
 *   <li>权重归档 {@code *.cact}，经 {@code -Dchua.needle.weights} 或
 *       {@code CHUA_NEEDLE_WEIGHTS} 指定；未配置时在引擎库同目录自动查找</li>
 * </ol>
 *
 * <p><b>典型用法：</b></p>
 * <pre>{@code
 * NeedleNative.init("你是家庭自动化助手", toolsJson, null);
 * String envelope = NeedleNative.complete("把客厅灯调到 30", 512);
 * }</pre>
 *
 * <p><b>线程安全但无并发吞吐：</b>引擎为进程级单例且权重不可卸载，
 * {@link NeedleEngine} 已将所有原生调用串行化；多线程并发调用会排队而非并行。
 * {@link #init} 在 system 与 tools 未变时幂等，可安全地每请求调用。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NeedleNative {

    static {
        NeedleNativeJniLoader.load();
    }

    private NeedleNative() {
    }

    /**
     * 引擎动态库是否加载成功且符号已解析。
     *
     * <p>本方法只读取加载器记录的状态，不触发任何原生调用，也不加载权重。</p>
     *
     * @return 加载成功返回 true
     */
    public static boolean isLoaded() {
        return NeedleNativeJniLoader.isLoaded();
    }

    /**
     * 引擎是否已具备推理条件（库已加载且权重已绑定）。
     *
     * @return 就绪返回 true
     */
    public static boolean isReady() {
        return NeedleNativeJniLoader.isLoaded() && NeedleEngine.isReady();
    }

    /**
     * 获取动态库加载失败原因。
     *
     * @return 失败异常；未尝试过或加载成功时返回 null
     */
    public static Throwable getLoadError() {
        return NeedleNativeJniLoader.loadError();
    }

    /**
     * 初始化引擎会话。
     *
     * <p>本方法会按需加载权重归档（每个 JVM 进程仅加载一次）。当 system 与 tools
     * 与当前绑定一致时直接返回，不重复调用引擎，因此可以每请求调用一次。</p>
     *
     * @param system 系统提示词，可为 null
     * @param tools  工具声明 JSON 字符串，可为 "[]" 空数组
     * @param model  模型名称；当前引擎由权重归档决定模型，保留此参数仅为兼容既有调用方
     * @throws IllegalStateException 动态库未加载、权重缺失或引擎返回错误时抛出
     */
    public static void init(String system, String tools, String model) {
        requireLibrary();
        NeedleEngine.init(system, tools);
    }

    /**
     * 完成一次对话生成。
     *
     * <p>必须在 {@link #init} 之后调用。多轮对话依赖引擎内部上下文，
     * 需要清空历史时调用 {@link #reset()}。</p>
     *
     * @param prompt    用户提示文本
     * @param maxTokens 最大生成令牌数
     * @return 引擎返回的 JSON envelope 原文
     * @throws IllegalStateException 未建立会话或引擎返回错误时抛出
     */
    public static String complete(String prompt, int maxTokens) {
        requireLibrary();
        return NeedleEngine.complete(prompt, maxTokens);
    }

    /**
     * 计算文本嵌入向量。
     *
     * <p>仅 Needle 3 引擎支持，Needle 2 引擎无 {@code needle_embed} 符号。</p>
     *
     * @param text 输入文本
     * @return 浮点向量
     * @throws UnsupportedOperationException 引擎无嵌入能力时抛出
     * @throws IllegalStateException           未建立会话或引擎返回错误时抛出
     */
    public static float[] embed(String text) {
        requireLibrary();
        return NeedleEngine.embed(text);
    }

    /**
     * 清空对话历史，保留工具声明与权重。
     */
    public static void reset() {
        requireLibrary();
        NeedleEngine.reset();
    }

    /**
     * 校验动态库可用性并给出明确失败原因。
     *
     * @throws IllegalStateException 动态库未加载时抛出
     */
    private static void requireLibrary() {
        if (isLoaded()) {
            return;
        }
        Throwable cause = getLoadError();
        throw new IllegalStateException("needle 引擎不可用："
                + (cause == null ? "动态库未加载" : cause.getMessage()), cause);
    }
}
