package com.chua.filesearch.support.bridge.jna;

import lombok.AllArgsConstructor;
import lombok.EqualsAndHashCode;
import lombok.Getter;
import lombok.ToString;

/**
 * 文件搜索单条结果（Java 8 数据载体）。
 *
 * <p>字段与 Java 25 侧 {@code RustFileSearchBridge.FileResultData} 对齐；因 Java 8
 * 不支持 {@code record}，此处以 {@code final} 类 + Lombok 实现只读语义。</p>
 *
 * <p>原生 JSON 仅提供 path / size / modified / ext 四项，其余字段按默认值填充：
 * {@code directory} 恒为 {@code false}，{@code attributes} / {@code usnRecordId} /
 * {@code parentFileId} / {@code allocatedSize} 恒为 {@code 0}。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Getter
@ToString
@EqualsAndHashCode
@AllArgsConstructor
public final class FileSearchResult {

    /**
     * 文件路径（原生已把反斜杠规范化为斜杠）
     */
    private final String path;

    /**
     * 文件字节数
     */
    private final long size;

    /**
     * 最后修改时间（Unix 毫秒）
     */
    private final long lastModified;

    /**
     * 是否目录（原生遍历跳过目录项，恒为 false）
     */
    private final boolean directory;

    /**
     * 扩展名（小写，无扩展名时为空串）
     */
    private final String extension;

    /**
     * 文件属性位（原生未提供，恒为 0）
     */
    private final int attributes;

    /**
     * USN 记录号（原生未提供，恒为 0）
     */
    private final long usnRecordId;

    /**
     * 父目录文件号（原生未提供，恒为 0）
     */
    private final long parentFileId;

    /**
     * 分配大小（原生未提供，恒为 0）
     */
    private final long allocatedSize;
}
