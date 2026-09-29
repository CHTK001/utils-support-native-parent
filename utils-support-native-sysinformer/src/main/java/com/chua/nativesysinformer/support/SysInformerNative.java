package com.chua.nativesysinformer.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import lombok.extern.slf4j.Slf4j;

/**
 * {@code sysinformer} 原生库的 Java 侧 FFM 绑定。
 *
 * <p>原生库只暴露三个 C 符号：{@code sysinformer_call}（统一入口）、
 * {@code sysinformer_free_string}（释放返回值）、{@code sysinformer_version}。
 * 所有能力通过 {@code op} 名分发，新增能力不需要改本类的绑定。</p>
 *
 * <p><b>返回值一律是 JSON 信封</b>，形如
 * {@code {"ok":true,"data":...,"error":null}} 或
 * {@code {"ok":false,"data":null,"error":"原因"}}。平台不支持某项能力时会返回
 * {@code ok=false} 并给出具体原因，<b>不会用空集合冒充</b>，调用方据此可区分
 * "不支持"与"支持但结果为空"。</p>
 *
 * <p><b>线程安全</b>：原生侧句柄为不可变，本类的静态句柄加载一次后只读，
 * 可并发调用。但原生侧内部有少量全局状态（如事件订阅），并发语义见各 op 说明。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class SysInformerNative {

    /**
     * 原生库逻辑名，与 Rust 侧 {@code [lib] name} 一致。
     */
    private static final String LIBRARY_NAME = "sysinformer";

    /**
     * 读取 C 字符串时给出的上界（字节）。
     *
     * <p>downcall 以 {@code ValueLayout.ADDRESS} 返回的是<b>零长</b> MemorySegment，
     * 直接 {@code getString(0)} 会抛 {@code IndexOutOfBoundsException: No null terminator found}
     * ——它拿不到扫描边界。必须先 {@code reinterpret} 出一个上界再读。
     * 原生侧用 {@code CString} 返回，必定以 NUL 结尾，所以只要上界大于实际长度即可
     * 正确停下；若响应异常大到超过该上界，会抛异常而不是读越界内存。</p>
     */
    private static final long CSTR_BOUND = 1L << 28;

    /**
     * JSON 解析器。
     */
    private static final ObjectMapper MAPPER = new ObjectMapper();

    /**
     * {@code sysinformer_call(op, args)} 的句柄。
     */
    private static final MethodHandle CALL;

    /**
     * {@code sysinformer_free_string(ptr)} 的句柄。
     */
    private static final MethodHandle FREE;

    /**
     * {@code sysinformer_version()} 的句柄。
     */
    private static final MethodHandle VERSION;

    /**
     * 加载原生库并绑定三个导出。
     */
    static {
        NativeLoader.of(LIBRARY_NAME)
                .glob(NativeUtils.getLibraryFileName(LIBRARY_NAME))
                .toTarget(NativeUtils.tempRoot().resolve(LIBRARY_NAME).toFile().getAbsolutePath())
                .load();
        SymbolLookup lookup = SymbolLookup.loaderLookup();
        Linker linker = Linker.nativeLinker();
        CALL = linker.downcallHandle(
                lookup.find("sysinformer_call")
                        .orElseThrow(() -> new UnsatisfiedLinkError("符号未找到: sysinformer_call")),
                FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        FREE = linker.downcallHandle(
                lookup.find("sysinformer_free_string")
                        .orElseThrow(() -> new UnsatisfiedLinkError("符号未找到: sysinformer_free_string")),
                FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
        VERSION = linker.downcallHandle(
                lookup.find("sysinformer_version")
                        .orElseThrow(() -> new UnsatisfiedLinkError("符号未找到: sysinformer_version")),
                FunctionDescriptor.of(ValueLayout.ADDRESS));
    }

    /**
     * 工具类，禁止实例化。
     */
    private SysInformerNative() {
    }

    /**
     * 读一个零长原生指针指向的 C 字符串，随后释放它。
     *
     * @param ptr 原生库返回的指针，可为 {@code MemorySegment.NULL}
     * @param arena 承载 reinterpret 边界的 Arena，不允许为 null
     * @return 字符串；指针为空时返回 null
     */
    private static String readAndFree(MemorySegment ptr, Arena arena) {
        if (ptr == null || ptr.equals(MemorySegment.NULL)) {
            return null;
        }
        try {
            return ptr.reinterpret(CSTR_BOUND, arena, null).getString(0);
        } finally {
            try {
                FREE.invoke(ptr);
            } catch (Throwable e) {
                log.warn("sysinformer 释放原生字符串失败: {}", e.toString());
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
     * 调用一个 op。
     *
     * @param op 操作名，不允许为 null
     * @param argsJson JSON 对象形式的参数，传 null 表示无参
     * @return 原始 JSON 信封字符串
     */
    public static String call(String op, String argsJson) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment opSeg = arena.allocateFrom(op, StandardCharsets.UTF_8);
            MemorySegment argSeg = argsJson == null
                    ? MemorySegment.NULL
                    : arena.allocateFrom(argsJson, StandardCharsets.UTF_8);
            MemorySegment ptr = (MemorySegment) CALL.invoke(opSeg, argSeg);
            return readAndFree(ptr, arena);
        } catch (Throwable e) {
            // 不把异常抛给业务：返回与原生侧同构的信封，调用方只需处理一种失败形态
            throw new IllegalStateException("sysinformer 调用失败: " + op, e);
        }
    }

    /**
     * 调用 op 并把结果解析成 JSON 树。
     *
     * @param op 操作名，不允许为 null
     * @param argsJson JSON 对象形式的参数，传 null 表示无参
     * @return 解析后的 JSON 根节点
     */
    public static JsonNode callJson(String op, String argsJson) {
        String raw = call(op, argsJson);
        try {
            return MAPPER.readTree(raw == null ? "{}" : raw);
        } catch (Exception e) {
            throw new IllegalStateException("sysinformer 返回的 JSON 无法解析: " + op, e);
        }
    }

    /**
     * 调用 op 并返回信封里的 {@code data} 节点。
     *
     * @param op 操作名，不允许为 null
     * @param argsJson JSON 参数，传 null 表示无参
     * @return {@code data} 节点；{@code ok=false} 或没有 data 时返回 {@code NullNode}
     */
    public static JsonNode callData(String op, String argsJson) {
        JsonNode root = callJson(op, argsJson);
        JsonNode ok = root.get("ok");
        if (ok == null || !ok.asBoolean()) {
            JsonNode err = root.get("error");
            log.debug("sysinformer {} 未成功: {}", op, err == null ? "(无原因)" : err.asText());
            return MAPPER.nullNode();
        }
        JsonNode data = root.get("data");
        return data == null ? MAPPER.nullNode() : data;
    }

    /**
     * 上一次调用是否成功。用于在不解析整个 JSON 时快速判断。
     *
     * @param rawJson {@link #call(String)} 的返回值
     * @return 成功返回 true，否则 false
     */
    public static boolean isOk(String rawJson) {
        if (rawJson == null) {
            return false;
        }
        try {
            JsonNode ok = MAPPER.readTree(rawJson).get("ok");
            return ok != null && ok.asBoolean();
        } catch (Exception e) {
            return false;
        }
    }

    /**
     * 从信封里取失败原因。
     *
     * @param rawJson {@link #call(String)} 的返回值
     * @return 失败原因；成功或无原因时返回 null
     */
    public static String errorOf(String rawJson) {
        if (rawJson == null) {
            return "原生库返回空指针";
        }
        try {
            JsonNode err = MAPPER.readTree(rawJson).get("error");
            return err == null || err.isNull() ? null : err.asText();
        } catch (Exception e) {
            return "无法解析信封: " + e.getMessage();
        }
    }

    /**
     * 原生库版本与构建目标。
     *
     * @return 形如 {@code {"version":"0.1.0","platform":"windows","target":"x86_64"}} 的 JSON
     */
    public static String version() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment ptr = (MemorySegment) VERSION.invoke();
            return readAndFree(ptr, arena);
        } catch (Throwable e) {
            throw new IllegalStateException("sysinformer 取版本失败", e);
        }
    }

    /**
     * 当前平台标识。
     *
     * @return windows / linux / macos / unknown
     */
    public static String platform() {
        String raw = version();
        try {
            JsonNode p = MAPPER.readTree(raw == null ? "{}" : raw).get("platform");
            return p == null ? "unknown" : p.asText();
        } catch (Exception e) {
            return "unknown";
        }
    }

    /**
     * 系统级快照（CPU / 内存 / 磁盘 / 网络 / 负载 / 主机 / 平台相关项）。
     *
     * @return 快照 JSON 节点
     */
    public static JsonNode systemSnapshot() {
        return callData("system.snapshot", null);
    }

    /**
     * 完整进程列表（不再是截断的前 N 名）。
     *
     * @return 进程数组节点
     */
    public static JsonNode processList() {
        return callData("process.list", null);
    }

    /**
     * 进程树。
     *
     * @return 根节点数组
     */
    public static JsonNode processTree() {
        return callData("process.tree", null);
    }

    /**
     * 单个进程详情。
     *
     * @param pid 进程 ID
     * @return 进程详情节点
     */
    public static JsonNode processDetail(int pid) {
        return callData("process.detail", "{\"pid\":" + pid + "}");
    }

    /**
     * 线程列表（按需调用，不参与周期采样）。
     *
     * @param pid 进程 ID
     * @return 线程数组节点
     */
    public static JsonNode threads(int pid) {
        return callData("process.threads", "{\"pid\":" + pid + "}");
    }

    /**
     * 环境变量（按需调用）。失败时 {@code data} 为 null，原因见 {@link #errorOf(String)}。
     *
     * @param pid 进程 ID
     * @return 环境变量数组节点
     */
    public static JsonNode envVars(int pid) {
        return callData("process.env", "{\"pid\":" + pid + "}");
    }

    /**
     * 句柄 / fd 列表（按需调用）。
     *
     * @param pid 进程 ID
     * @return 句柄数组节点
     */
    public static JsonNode handles(int pid) {
        return callData("process.handles", "{\"pid\":" + pid + "}");
    }

    /**
     * 已加载模块列表（按需调用）。
     *
     * @param pid 进程 ID
     * @return 模块数组节点
     */
    public static JsonNode modules(int pid) {
        return callData("process.modules", "{\"pid\":" + pid + "}");
    }

    /**
     * 令牌 / 凭据（按需调用）。
     *
     * @param pid 进程 ID
     * @return 凭据节点
     */
    public static JsonNode credential(int pid) {
        return callData("process.credential", "{\"pid\":" + pid + "}");
    }

    /**
     * 内存映射（按需调用）。
     *
     * @param pid 进程 ID
     * @return 映射数组节点
     */
    public static JsonNode mappings(int pid) {
        return callData("process.mappings", "{\"pid\":" + pid + "}");
    }

    /**
     * 栈回溯（按需调用）。
     *
     * @param pid 进程 ID
     * @param tid 线程 ID，传 0 表示进程主线程
     * @param kernel 是否取内核态栈
     * @return 栈节点
     */
    public static JsonNode stackTrace(int pid, long tid, boolean kernel) {
        return callData("process.stack",
                "{\"pid\":" + pid + ",\"tid\":" + tid + ",\"kernel\":" + kernel + "}");
    }

    /**
     * 套接字 / 连接列表。
     *
     * @param pid 进程 ID；传 0 表示全系统
     * @return 连接数组节点
     */
    public static JsonNode sockets(int pid) {
        return callData("socket.list", pid > 0 ? "{\"pid\":" + pid + "}" : "{}");
    }

    /**
     * 服务列表。
     *
     * @return 服务数组节点
     */
    public static JsonNode services() {
        return callData("service.list", null);
    }

    /**
     * 内核模块 / 驱动列表。
     *
     * @return 模块数组节点
     */
    public static JsonNode kernelModules() {
        return callData("kernel.modules", null);
    }

    /**
     * 执行一个控制动作（终止 / 挂起 / 恢复 / 优先级 / 亲和性 / 关句柄）。
     *
     * <p>本模块<b>不提供注入类能力</b>——它是恶意软件技术，且不属于指标采集。</p>
     *
     * @param kind 动作名，见 README 的 op 一览表
     * @param target 目标 ID
     * @param arg 动作参数，无参数传 0
     * @return 动作结果节点
     */
    public static JsonNode action(String kind, String target, long arg) {
        return callData("action.exec",
                "{\"kind\":\"" + kind + "\",\"target\":\"" + target + "\",\"arg\":" + arg + "}");
    }

    /**
     * 开始事件订阅（事件驱动，D1）。
     *
     * @param mask 事件类型位掩码，传 -1 表示全部
     * @return 启动结果节点
     */
    public static JsonNode startEvents(int mask) {
        return callData("events.start", "{\"mask\":" + mask + "}");
    }

    /**
     * 取出已缓存的事件。
     *
     * @return 事件数组节点
     */
    public static JsonNode pollEvents() {
        return callData("events.poll", null);
    }

    /**
     * 停止事件订阅。
     *
     * @return 停止结果节点
     */
    public static JsonNode stopEvents() {
        return callData("events.stop", null);
    }
}
