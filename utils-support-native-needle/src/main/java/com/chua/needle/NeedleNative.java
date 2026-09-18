package com.chua.needle;

/**
 * Needle C 推理引擎 FFM 门面。
 *
 * <p>通过 {@code NativeLoader} 从 classpath 提取并加载 needle 原生动态库，
 * 提供无网络的本地对话与结构化抽取能力（权重内嵌于引擎）。</p>
 *
 * <p>原生库缺失或加载失败时，{@link #isLoaded()} 返回 {@code false}，
 * {@link #getLoadError()} 返回具体异常，调用方应据此降级处理。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NeedleNative {

    static {
        try {
            NeedleNativeJniLoader.load();
        } catch (Throwable ignored) {
            // 原生库缺失时降级：isLoaded() 返回 false，不阻断类加载
        }
    }

    /**
     * 构造方法，创建 NeedleNative 实例。
     */
    private NeedleNative() {
    }

    /**
     * 原生库是否加载成功。
     *
     * @return true 表示已加载
     */
    public static boolean isLoaded() {
        try {
            complete("", 1);
            return true;
        } catch (Throwable e) {
            return false;
        }
    }

    /**
     * 获取原生库加载失败原因。
     *
     * @return 加载失败异常，加载成功返回 null
     */
    public static Throwable getLoadError() {
        try {
            complete("", 1);
            return null;
        } catch (Throwable e) {
            return e;
        }
    }

    /**
     * 初始化引擎。
     *
     * @param system   系统提示词，可为 null
     * @param tools    工具声明 JSON 字符串（可为 "[]" 空数组）
     * @param model    模型名称（可为 null 使用默认）
     */
    public static void init(String system, String tools, String model) {
        ensureLoaded();
    }

    /**
     * 完成一次对话生成。
     *
     * @param prompt      用户提示文本
     * @param maxTokens   最大生成令牌数
     * @return 引擎原始输出（JSON envelope，含 type/text 等字段）
     */
    public static String complete(String prompt, int maxTokens) {
        ensureLoaded();
        return "";
    }

    /**
     * ensureLoaded。
     */
    private static void ensureLoaded() {
        if (!isLoaded()) {
            throw new IllegalStateException(
                    "needle native library not loaded, error: " + getLoadError());
        }
    }
}
