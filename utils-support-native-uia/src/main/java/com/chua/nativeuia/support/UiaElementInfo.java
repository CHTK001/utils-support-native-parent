package com.chua.nativeuia.support;

import com.fasterxml.jackson.annotation.JsonAutoDetect;
import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.PropertyAccessor;
import lombok.Data;
import lombok.experimental.Accessors;

/**
 * UIA 元素属性快照。
 *
 * <p>对应原生层 {@code ElementInfo}，由 {@code uia_describe} 批量返回。
 * 这是上层做"这个元素是不是我要找的东西"判断的全部依据。</p>
 *
 * <p>注意：本类用 {@code @Accessors(chain = true)} 提供链式 API，而链式 setter
 * 返回值不是 {@code void}，Jackson 默认不把它识别为 setter。因此这里显式要求
 * Jackson 直接绑定字段，否则反序列化会静默得到全 null 的对象。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Data
@Accessors(chain = true)
@JsonIgnoreProperties(ignoreUnknown = true)
@JsonAutoDetect(
        fieldVisibility = JsonAutoDetect.Visibility.ANY,
        getterVisibility = JsonAutoDetect.Visibility.NONE,
        isGetterVisibility = JsonAutoDetect.Visibility.NONE,
        setterVisibility = JsonAutoDetect.Visibility.NONE,
        creatorVisibility = JsonAutoDetect.Visibility.NONE)
public class UiaElementInfo {

    /**
     * 元素在原生元素池中的句柄（从 1 开始，0 表示无效）
     */
    private Long id;

    /**
     * 控件类型名
     */
    private String controlType;

    /**
     * 控件名称
     */
    private String name;

    /**
     * AutomationId
     */
    private String automationId;

    /**
     * 窗口类名
     */
    private String className;

    /**
     * 宿主进程 ID
     */
    private Integer processId;

    /**
     * 宿主进程名
     */
    private String processName;

    /**
     * 控件原生窗口句柄（无则为 0）
     */
    private Long nativeHandle;

    /**
     * 是否可用
     */
    private Boolean enabled;

    /**
     * 是否在屏
     */
    private Boolean offscreen;

    /**
     * {@code Value} 模式的当前值（不支持时为空）
     */
    private String value;

    /**
     * 文本是否只读
     */
    private Boolean readOnly;

    /**
     * 支持的控制模式名列表
     */
    private java.util.List<String> patterns;

    /**
     * 屏幕物理坐标矩形
     */
    private Rect rect;

    /**
     * 判断元素是否支持指定控制模式。
     *
     * @param patternName 模式名，如 {@code Value} / {@code Invoke}
     * @return 支持返回 true
     */
    public boolean supports(String patternName) {
        return patterns != null && patterns.contains(patternName);
    }

    /**
     * 判断元素是否可见可用（在屏且可用）。
     *
     * @return 可见可用返回 true
     */
    public boolean isVisible() {
        return Boolean.TRUE.equals(enabled) && !Boolean.TRUE.equals(offscreen);
    }

    /**
     * 屏幕物理坐标矩形。
     */
    @Data
    @Accessors(chain = true)
    @JsonIgnoreProperties(ignoreUnknown = true)
    @JsonAutoDetect(
            fieldVisibility = JsonAutoDetect.Visibility.ANY,
            getterVisibility = JsonAutoDetect.Visibility.NONE,
            isGetterVisibility = JsonAutoDetect.Visibility.NONE,
            setterVisibility = JsonAutoDetect.Visibility.NONE,
            creatorVisibility = JsonAutoDetect.Visibility.NONE)
    public static class Rect {

        /**
         * 左边界
         */
        @JsonProperty("left")
        private Integer left;

        /**
         * 上边界
         */
        @JsonProperty("top")
        private Integer top;

        /**
         * 右边界
         */
        @JsonProperty("right")
        private Integer right;

        /**
         * 下边界
         */
        @JsonProperty("bottom")
        private Integer bottom;

        /**
         * 判断矩形是否有效（宽高均为正）。
         *
         * @return 有效返回 true
         */
        public boolean isValid() {
            return left != null && top != null && right != null && bottom != null
                    && right > left && bottom > top;
        }

        /**
         * 计算矩形中心点。
         *
         * @return 长度为 2 的数组 {@code [x, y]}
         */
        public int[] center() {
            return new int[]{(left + right) / 2, (top + bottom) / 2};
        }
    }
}
