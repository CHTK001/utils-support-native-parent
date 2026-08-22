package com.chua.common.support.nativehttp;

import com.chua.common.support.utils.NativeLoader;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;

/**
 * Rust HTTP 桥接的 Panama FFM 绑定。
 *
 * <p>负责加载 {@code rust_http_bridge.dll/.so}（Rust hyper HTTP 服务器），
 * 并封装 {@code rhb_start / rhb_stop / rhb_poll_request / rhb_send_response}
 * 四个 C ABI 函数。请求信封格式：</p>
 *
 * <pre>
 * req_id(8, u64 LE) | method_len(2, u16 LE) | path_len(2, u16 LE)
 * | body_len(4, u32 LE) | method | path | body
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class RustHttpBridge implements AutoCloseable {

    /** 原生链接器 */
    private static final Linker LINKER = Linker.nativeLinker();

    /** rhb_start 句柄 */
    private static MethodHandle rhbStart;

    /** rhb_stop 句柄 */
    private static MethodHandle rhbStop;

    /** rhb_poll_request 句柄 */
    private static MethodHandle rhbPollRequest;

    /** rhb_send_response 句柄 */
    private static MethodHandle rhbSendResponse;

    static {
        // 使用 NativeLoader 统一加载机制：
        // 1. 从 classpath:/native/{platformDir}/ 抽取到目标目录
        // 2. MD5 校验避免重复拷贝
        // 3. 按文件名排序后 System.load
        NativeLoader.of("rust-http-bridge")
                .toTarget(Path.of(System.getProperty("java.io.tmpdir"), "chua-native", "rust-http-bridge"))
                .glob("*rust_http_bridge*")
                .load();

        // 获取已加载库的路径并绑定方法句柄
        String libName = System.mapLibraryName("rust_http_bridge");
        Path libPath = Path.of(System.getProperty("java.io.tmpdir"), "chua-native", "rust-http-bridge", libName);

        SymbolLookup lookup = SymbolLookup.libraryLookup(libPath, Arena.ofAuto());

        rhbStart = LINKER.downcallHandle(lookup.find("rhb_start").orElseThrow(),
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                        ValueLayout.JAVA_INT, ValueLayout.JAVA_INT));

        rhbStop = LINKER.downcallHandle(lookup.find("rhb_stop").orElseThrow(),
                FunctionDescriptor.ofVoid());

        rhbPollRequest = LINKER.downcallHandle(lookup.find("rhb_poll_request").orElseThrow(),
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_INT));

        rhbSendResponse = LINKER.downcallHandle(lookup.find("rhb_send_response").orElseThrow(),
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.JAVA_LONG, ValueLayout.JAVA_SHORT,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                        ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
    }

    /** 已启动标志 */
    private volatile boolean started;

    /** 响应队列满时的最大重试次数（onSpinWait 级，约百微秒内） */
    private static final int SEND_FULL_MAX_RETRY = 200_000;

    /** 轮询线程复用的 Arena（仅单条轮询线程使用） */
    private Arena pollArena;

    /** 轮询线程复用的原生缓冲段 */
    private MemorySegment pollSeg;

    private RustHttpBridge() {
    }

    /**
     * 启动 HTTP 桥接（Rust hyper 开始监听）。
     *
     * @param port      监听端口
     * @param shmName   共享内存对象名（内部自动追加 _req / _resp 后缀）
     * @param capacity  队列槽位数
     * @param slotSize  队列每槽字节数（请求信封 + 响应信封均不得超过）
     * @return 桥接实例
     */
    public static RustHttpBridge start(int port, String shmName, int capacity, int slotSize) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment nameSeg = arena.allocateFrom(shmName);
            int rc = (int) rhbStart.invokeExact(port, nameSeg, capacity, slotSize);
            if (rc != 0) {
                throw new IllegalStateException("rhb_start 失败, rc=" + rc);
            }
        } catch (Throwable e) {
            if (e instanceof RuntimeException re) {
                throw re;
            }
            throw new IllegalStateException("rhb_start 异常: " + e.getMessage(), e);
        }
        RustHttpBridge bridge = new RustHttpBridge();
        bridge.started = true;
        return bridge;
    }

    /**
     * 轮询请求队列，取出一条请求信封。
     *
     * @param out 输出缓冲（大小应不小于 slotSize - 8）
     * @return 信封字节数（&gt;0 有数据；0 无数据；&lt;0 错误）
     */
    public int pollRequest(byte[] out) {
        if (!started) {
            throw new IllegalStateException("桥接未启动");
        }
        try {
            // 复用轮询 Arena/段：pollRequest 仅由单条轮询线程调用（confined arena 安全），
            // 避免每请求新建 Arena + 分配 slotSize 段，减少 FFM 侧开销。
            if (pollArena == null) {
                pollArena = Arena.ofShared();
            }
            if (pollSeg == null || pollSeg.byteSize() != out.length) {
                pollSeg = pollArena.allocate(out.length);
            }
            int n = (int) rhbPollRequest.invokeExact(pollSeg, out.length);
            if (n > 0) {
                MemorySegment.copy(pollSeg, ValueLayout.JAVA_BYTE, 0, out, 0, n);
            }
            return n;
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_poll_request 异常: " + e.getMessage(), e);
        }
    }

    /**
     * 写入响应信封到响应队列。
     *
     * @param reqId       请求 ID（从请求信封前 8 字节读取）
     * @param status      HTTP 状态码
     * @param contentType Content-Type（可为 null）
     * @param body        响应体（可为 null）
     */
    public void sendResponse(long reqId, int status, String contentType, byte[] body) {
        if (!started) {
            throw new IllegalStateException("桥接未启动");
        }
        byte[] ct = contentType == null ? new byte[0] : contentType.getBytes(StandardCharsets.UTF_8);
        byte[] payload = body == null ? new byte[0] : body;
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment ctSeg = arena.allocate(ct.length);
            if (ct.length > 0) {
                MemorySegment.copy(ct, 0, ctSeg, ValueLayout.JAVA_BYTE, 0, ct.length);
            }
            MemorySegment bodySeg = arena.allocate(payload.length);
            if (payload.length > 0) {
                MemorySegment.copy(payload, 0, bodySeg, ValueLayout.JAVA_BYTE, 0, payload.length);
            }
            int rc = 0;
            // 队列满（rc=-8）为瞬态：虚拟线程突发写响应可能短暂超过 Rust 读取速率，
            // 有界自旋重试吸收突发，避免响应丢失导致客户端 504。
            int attempt = 0;
            while (true) {
                rc = (int) rhbSendResponse.invokeExact(reqId, (short) status, ctSeg, ct.length, bodySeg, payload.length);
                if (rc != -8 || attempt >= SEND_FULL_MAX_RETRY) {
                    break;
                }
                Thread.onSpinWait();
                attempt++;
            }
            if (rc != 0) {
                throw new IllegalStateException("rhb_send_response 失败, rc=" + rc);
            }
        } catch (Throwable e) {
            if (e instanceof RuntimeException re) {
                throw re;
            }
            throw new IllegalStateException("rhb_send_response 异常: " + e.getMessage(), e);
        }
    }

    /**
     * 停止桥接并回收线程/队列。
     */
    public void stop() {
        if (!started) {
            return;
        }
        started = false;
        try {
            rhbStop.invokeExact();
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_stop 异常: " + e.getMessage(), e);
        } finally {
            if (pollArena != null) {
                pollArena.close();
                pollArena = null;
                pollSeg = null;
            }
        }
    }

    @Override
    public void close() {
        stop();
    }
}
