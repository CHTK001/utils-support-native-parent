package com.chua.filesearch.support.bridge.jna;

import com.sun.jna.Library;
import com.sun.jna.Pointer;

/**
 * {@code file_search} 动态库扁平 C ABI 的 JNA 映射。
 *
 * <p>与 Java 25 侧 {@code RustFileSearchBridge} 走的是同一组符号：{@code Java_*} 前缀的
 * 两个函数并<strong>不符合</strong> JNI 约定（无 {@code JNIEnv*} / {@code jclass} 前导参数），
 * 本质是返回 JSON 的普通 C 函数，因此 JNA 按普通导出函数绑定即可，绝不能按 JNI 方式调用。</p>
 *
 * <p>方法名直接取原生符号全名，是因为 JNA 默认以方法名查符号。原生用
 * {@code extern "system"} 导出，在 64 位（x86_64 / aarch64）平台上与 {@code cdecl}
 * 调用约定一致，本模块已提交的产物全部是 64 位，故无需 {@code StdCallLibrary}。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public interface FileSearchAbi extends Library {

    /**
     * 读取原生库版本号。
     *
     * <p>返回指向静态常量字符串的指针，<strong>不可</strong>释放。</p>
     *
     * @return 版本字符串指针
     */
    Pointer fast_get_version();

    /**
     * 取消正在进行的搜索（当前原生实现为空操作）。
     */
    void fast_search_cancel();

    /**
     * 按名称通配符搜索并返回 JSON 字符串。
     *
     * <p>返回指针由 Rust {@code CString::into_raw} 分配，跨分配器释放不安全，
     * 调用方不释放（与 FFM 版行为一致）。</p>
     *
     * @param rootPath    根目录的 UTF-8 C 字符串
     * @param namePattern 名称通配符的 UTF-8 C 字符串；为 {@code null} 时传 C 空指针
     * @param maxResults  最大返回数，&lt;= 0 表示不限
     * @return JSON 字符串指针；根目录为空指针时返回错误 JSON
     */
    Pointer Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName(
            Pointer rootPath,
            Pointer namePattern,
            int maxResults);

    /**
     * 遍历目录树并返回 JSON 字符串。
     *
     * @param rootPath   根目录的 UTF-8 C 字符串
     * @param maxDepth   最大深度（原生侧硬编码为 3，此参数被忽略）
     * @param maxResults 最大返回数，&lt;= 0 表示不限
     * @return JSON 字符串指针；根目录为空指针时返回错误 JSON
     */
    Pointer Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawGetTree(
            Pointer rootPath,
            int maxDepth,
            int maxResults);
}
