package com.chua.needle;

/**
 * Needle 推理引擎 FFM 门面。
 *
 * <p><b>当前状态：原生库与 FFM 绑定均未实现。</b>
 * {@code chua_native_needle} 的二进制不在本仓库，且本门面尚未声明任何
 * {@code MethodHandle} 绑定，因此即便补齐动态库也还不能真正推理。
 * 保留本类是为了固定对外接口形态，待原生侧就绪后补齐绑定即可，
 * 调用方（{@code NeedleChatClient}）无需改动。</p>
 *
 * <p>使用前应先 {@link #isLoaded()} 判断，避免在未部署时抛出异常：</p>
 * <pre>{@code
 * if (!NeedleNative.isLoaded()) {
 *     log.warn("needle 未就绪: {}", NeedleNative.getLoadError().getMessage());
 *     return;
 * }
 * }</pre>
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
     * 原生库是否加载成功。
     *
     * <p>本方法只读取加载器记录的状态，不触发任何原生调用。</p>
     *
     * @return 加载成功返回 true
     */
    public static boolean isLoaded() {
        return NeedleNativeJniLoader.isLoaded();
    }

    /**
     * 获取原生库加载失败原因。
     *
     * @return 失败异常；未尝试过或加载成功时返回 null
     */
    public static Throwable getLoadError() {
        return NeedleNativeJniLoader.loadError();
    }

    /**
     * 初始化引擎。
     *
     * @param system 系统提示词，可为 null
     * @param tools  工具声明 JSON 字符串（可为 "[]" 空数组）
     * @param model  模型名称，可为 null 使用默认
     * @throws IllegalStateException        原生库未加载时抛出
     * @throws UnsupportedOperationException 库已加载但 FFM 绑定未实现时抛出
     */
    public static void init(String system, String tools, String model) {
        requireBinding();
    }

    /**
     * 完成一次对话生成。
     *
     * @param prompt    用户提示文本
     * @param maxTokens 最大生成令牌数
     * @return 引擎原始输出（JSON envelope）
     * @throws IllegalStateException        原生库未加载时抛出
     * @throws UnsupportedOperationException 库已加载但 FFM 绑定未实现时抛出
     */
    public static String complete(String prompt, int maxTokens) {
        requireBinding();
        return "";
    }

    /**
     * 校验可用性并给出明确失败原因。
     *
     * @throws IllegalStateException        原生库未加载时抛出
     * @throws UnsupportedOperationException 库已加载但 FFM 绑定未实现时抛出
     */
    private static void requireBinding() {
        if (!isLoaded()) {
            Throwable cause = getLoadError();
            throw new IllegalStateException("needle 原生库不可用：" + cause.getMessage(), cause);
        }
        throw new UnsupportedOperationException(
                "needle 原生库已加载，但 FFM 绑定尚未实现；本门面当前不含任何 MethodHandle 绑定，"
                        + "无法执行推理。请勿在未就绪时把返回空串当作推理结果。");
    }
}
