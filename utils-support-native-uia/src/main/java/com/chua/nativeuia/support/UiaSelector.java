package com.chua.nativeuia.support;

import com.fasterxml.jackson.annotation.JsonInclude;
import lombok.Data;
import lombok.experimental.Accessors;

import java.util.ArrayList;
import java.util.List;

/**
 * UIA 元素选择器：描述"如何定位一个控件"，序列化后交给原生层做匹配。
 *
 * <p>之所以把定位策略做成数据而非硬编码，是因为 IM 类客户端的控件层级会随版本漂移
 * （微信 3.9 的原生 Qt 控件树与 4.x 的 Electron 树完全不同）。选择器外置后，
 * 调整定位只需改配置，不必重编动态库。</p>
 *
 * <p>所有字符串字段留空表示"不约束该维度"。</p>
 *
 * <h3>典型用法</h3>
 * <pre>{@code
 * // 找最后一个可见的 ListItem（会话列表项）
 * UiaSelector.of("ListItem")
 *         .requireEnabled(true)
 *         .requireOnscreen(true)
 *         .index(-1)
 *         .toJson();
 *
 * // 找名字匹配正则、且 4 层内含 Text 子控件的 ListItem（消息项）
 * UiaSelector.of("ListItem")
 *         .nameRegex("^\\(3\\)张三$")
 *         .children(List.of(UiaSelector.of("Text")))
 *         .childDepth(4)
 *         .toJson();
 * }</pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Data
@Accessors(chain = true)
@JsonInclude(JsonInclude.Include.NON_NULL)
public class UiaSelector {

    /**
     * 控件类型名，如 {@code ListItem} / {@code Edit} / {@code Text} / {@code Button}
     */
    private String controlType;

    /**
     * 控件名称精确匹配
     */
    private String name;

    /**
     * 控件名称正则匹配（Java {@code Pattern} 语法，原生侧按 Rust {@code regex} 编译，
     * 不支持反向引用与环视）
     */
    private String nameRegex;

    /**
     * AutomationId 精确匹配
     */
    private String automationId;

    /**
     * 窗口类名精确匹配
     */
    private String className;

    /**
     * 宿主进程 ID 精确匹配
     */
    private Integer processId;

    /**
     * 是否要求控件可用
     */
    private Boolean requireEnabled;

    /**
     * 是否要求控件在屏（未被滚动裁剪）
     */
    private Boolean requireOnscreen;

    /**
     * 相对根元素的遍历最大深度，缺省为不限
     */
    private Integer maxDepth;

    /**
     * 必须存在的后代选择器（全部满足才算命中）
     */
    private List<UiaSelector> children;

    /**
     * 向上查找时允许跨越的层级上限，配合 {@link #ancestor} 使用
     */
    private Integer childDepth;

    /**
     * 祖先选择器（满足即命中）
     */
    private UiaSelector ancestor;

    /**
     * 命中后取第几个；{@code null} 表示全部返回，{@code -1} 表示最后一个
     */
    private Integer index;

    /**
     * 以控件类型创建选择器。
     *
     * @param controlType 控件类型名
     * @return 选择器实例
     */
    public static UiaSelector of(String controlType) {
        return new UiaSelector().setControlType(controlType);
    }

    /**
     * 以控件类型与名称精确匹配创建选择器。
     *
     * @param controlType 控件类型名
     * @param name        控件名称
     * @return 选择器实例
     */
    public static UiaSelector of(String controlType, String name) {
        return new UiaSelector().setControlType(controlType).setName(name);
    }

    /**
     * 追加一个必须存在的后代选择器。
     *
     * @param child 后代选择器
     * @return 自身
     */
    public UiaSelector withChild(UiaSelector child) {
        if (children == null) {
            children = new ArrayList<>(4);
        }
        children.add(child);
        return this;
    }

    /**
     * 序列化为原生层可解析的 JSON。
     *
     * <p>原生层的解析失败会在 {@code uia_find} 时以错误码返回，因此这里
     * 只保证语法正确，不做选择器语义校验。</p>
     *
     * @return JSON 字符串
     */
    public String toJson() {
        return UiaJson.write(this);
    }

    /**
     * 从原生层返回的 JSON 数组反序列化为选择器列表（供选择器模板热更新使用）。
     *
     * @param json JSON 数组字符串
     * @return 选择器列表
     */
    public static List<UiaSelector> listFromJson(String json) {
        return UiaJson.readList(json);
    }
}
