package com.chua.nativesysinformer.java8;

import com.sun.jna.NativeLibrary;
import com.sun.jna.Pointer;
import java.io.IOException;
import java.nio.file.Path;
import java.util.logging.Level;
import java.util.logging.Logger;

/**
 * {@code sysinformer} 原生库的 Java 8 JNA 绑定。
 *
 * <p>原生库只暴露三个 C 符号：{@code sysinformer_call}（统一入口）、
 * {@code sysinformer_free_string}（释放返回值）、{@code sysinformer_version}。
 * 所有能力通过 {@code op} 名分发，新增能力不需要改本类的绑定，
 * 与 Java 25 侧 {@code SysInformerNative}（Panama FFM）的 op 语义完全一致。</p>
 *
 * <p><b>返回值是 JSON 信封</b>：{@code {"ok":true,"data":...,"error":null}} 或
 * {@code {"ok":false,"data":null,"error":"原因"}}。平台不支持某项能力时返回
 * {@code ok=false} 并给出具体原因，不会用空集合冒充。</p>
 *
 * <p><b>为什么用 {@code Function} 动态取函数而不是定义 {@code Library} 接口</b>：
 * 本库只有三个导出且签名固定（{@code (char*,char*)->char*} 等），用
 * {@code NativeLibrary.getFunction} 更直接，也避免为每个 op 生成一个接口方法。</p>
 *
 * <p><b>线程安全</b>：JNA 的 {@code Function.invoke*} 可并发调用；原生侧句柄在加载后
 * 只读。但 ETW 等能力在原生侧有全局状态，并发语义见各 op 说明。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class SysInformerJna {

    /**
     * 日志记录器。用 JDK 自带 logging 而不是 slf4j-api + 绑定，
     * 避免在最小 JDK 8 环境里因缺日志实现而多发一条告警。
     */
    private static final Logger LOG = Logger.getLogger(SysInformerJna.class.getName());

    /**
     * 已加载的原生库。加载一次后只读。
     */
    private static final NativeLibrary LIB;

    /**
     * 静态初始化：抽取并加载。
     */
    static {
        try {
            Path libPath = NativeLibraryExtractor.extract();
            LIB = NativeLibrary.getInstance(libPath.toAbsolutePath().toString());
            LOG.fine("sysinformer 原生库已加载: " + libPath);
        } catch (IOException e) {
            throw new IllegalStateException("sysinformer 原生库加载失败: " + e.getMessage(), e);
        }
    }

    /**
     * 工具类，禁止实例化。
     */
    private SysInformerJna() {
    }

    /**
     * 调用一个 op，返回原始 JSON 信封字符串。
     *
     * @param op 操作名，不允许为 null
     * @param argsJson JSON 对象形式的参数，传 null 表示无参
     * @return 原始 JSON 信封字符串
     */
    public static String call(String op, String argsJson) {
        Pointer p = null;
        try {
            p = LIB.getFunction("sysinformer_call")
                    .invokePointer(new Object[] {op, argsJson == null ? "{}" : argsJson});
            if (p == null) {
                return "{\"ok\":false,\"data\":null,\"error\":\"原生库返回空指针\"}";
            }
            return p.getString(0);
        } finally {
            if (p != null) {
                try {
                    LIB.getFunction("sysinformer_free_string").invokeVoid(new Object[] {p});
                } catch (Throwable e) {
                    LOG.log(Level.WARNING, "释放原生字符串失败", e);
                }
            }
        }
    }

    /**
     * 调用一个无参 op。
     *
     * @param op 操作名，不允许为 null
     * @return 原始 JSON 信封字符串
     */
    public static String call(String op) {
        return call(op, "{}");
    }

    /**
     * 原生库版本与构建目标。
     *
     * @return 形如 {@code {"version":"0.1.0","platform":"windows","target":"x86_64"}}
     */
    public static String version() {
        Pointer p = null;
        try {
            p = LIB.getFunction("sysinformer_version").invokePointer(new Object[0]);
            if (p == null) {
                return null;
            }
            return p.getString(0);
        } finally {
            if (p != null) {
                LIB.getFunction("sysinformer_free_string").invokeVoid(new Object[] {p});
            }
        }
    }

    /**
     * 上一次调用是否成功。
     *
     * <p>不引入 JSON 解析器：只需判断信封里的 {@code "ok":true}，用一次字符串匹配即可，
     * 这样 JDK 8 侧保持零第三方 JSON 依赖。</p>
     *
     * @param rawJson {@link #call(String)} 的返回值
     * @return 成功返回 true，否则 false
     */
    public static boolean isOk(String rawJson) {
        if (rawJson == null) {
            return false;
        }
        int i = rawJson.indexOf("\"ok\"");
        if (i < 0) {
            return false;
        }
        int t = rawJson.indexOf("true", i);
        int f = rawJson.indexOf("false", i);
        // 取离 "ok" 最近的那个布尔值
        return t >= 0 && (f < 0 || t < f);
    }

    /**
     * 从信封里取失败原因（不解析 JSON，直接从 {@code "error"} 后取字符串）。
     *
     * @param rawJson {@link #call(String)} 的返回值
     * @return 失败原因；成功或无原因时返回 null
     */
    public static String errorOf(String rawJson) {
        if (rawJson == null) {
            return "原生库返回空指针";
        }
        String key = "\"error\":";
        int i = rawJson.indexOf(key);
        if (i < 0) {
            return null;
        }
        int start = rawJson.indexOf('"', i + key.length());
        if (start < 0) {
            return rawJson.substring(i + key.length()).trim();
        }
        StringBuilder sb = new StringBuilder();
        for (int j = start + 1; j < rawJson.length(); j++) {
            char c = rawJson.charAt(j);
            if (c == '\\' && j + 1 < rawJson.length()) {
                sb.append(jsonEscape(rawJson.charAt(j + 1)));
                j++;
                continue;
            }
            if (c == '"') {
                break;
            }
            sb.append(c);
        }
        return sb.toString();
    }

    /**
     * 还原 JSON 转义字符。
     *
     * @param c 转义后的字符
     * @return 原字符
     */
    private static char jsonEscape(char c) {
        switch (c) {
            case 'n':
                return '\n';
            case 't':
                return '\t';
            case 'r':
                return '\r';
            case '"':
                return '"';
            case '\\':
                return '\\';
            default:
                return c;
        }
    }
}