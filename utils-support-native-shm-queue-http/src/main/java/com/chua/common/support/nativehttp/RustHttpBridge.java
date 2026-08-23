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
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * rust_http_bridge.dll 的 Panama FFM 绑定。
 *
 * <p>负责加载 {@code rust_http_bridge.dll/.so/.dylib}（Rust hyper HTTP 服务器），
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

    /** kernel32 符号查找 */
    private static final SymbolLookup KERNEL32;

    /** WaitForSingleObject 句柄 */
    private static final MethodHandle waitForSingleObject;

    static {
        SymbolLookup k32;
        MethodHandle wfso;
        try {
            k32 = SymbolLookup.libraryLookup("kernel32.dll", Arena.ofAuto());
            wfso = LINKER.downcallHandle(k32.find("WaitForSingleObject").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
        } catch (Throwable e) {
            k32 = null;
            wfso = null;
        }
        KERNEL32 = k32;
        waitForSingleObject = wfso;
    }

    /** 是否已加载 */
    private static volatile boolean loaded = false;

    /** 加载失败原因 */
    private static volatile String loadError;

    /** rhb_start 句柄 */
    private static MethodHandle rhbStart;

    /** rhb_stop 句柄 */
    private static MethodHandle rhbStop;

    /** rhb_poll_request 句柄 */
    private static MethodHandle rhbPollRequest;

    /** rhb_send_response 句柄 */
    private static MethodHandle rhbSendResponse;

    /** rhb_get_req_notify_fd 句柄 */
    private static MethodHandle rhbGetReqNotifyFd;

    /** rhb_get_resp_notify_fd 句柄 */
    private static MethodHandle rhbGetRespNotifyFd;

    /** rhb_get_req_shm_ptr 句柄 */
    private static MethodHandle rhbGetReqShmPtr;

    /** rhb_get_resp_shm_ptr 句柄 */
    private static MethodHandle rhbGetRespShmPtr;

    static {
        try {
            String libName = System.mapLibraryName("rust_http_bridge");
            Path nativeDir = Path.of(System.getProperty("java.io.tmpdir"), "chua-native", "rust-http-bridge");
            NativeLoader.of("rust-http-bridge")
                    .toTarget(nativeDir)
                    .glob("*rust_http_bridge*")
                    .load();
            Path libPath = nativeDir.resolve(libName);
            if (!Files.exists(libPath)) {
                throw new IllegalStateException("未找到原生库: " + libPath);
            }
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

            var reqNotifyFd = lookup.find("rhb_get_req_notify_fd");
            rhbGetReqNotifyFd = reqNotifyFd.isPresent()
                    ? LINKER.downcallHandle(reqNotifyFd.get(), FunctionDescriptor.of(ValueLayout.JAVA_INT))
                    : null;

            var respNotifyFd = lookup.find("rhb_get_resp_notify_fd");
            rhbGetRespNotifyFd = respNotifyFd.isPresent()
                    ? LINKER.downcallHandle(respNotifyFd.get(), FunctionDescriptor.of(ValueLayout.JAVA_INT))
                    : null;

            var reqShmPtr = lookup.find("rhb_get_req_shm_ptr");
            rhbGetReqShmPtr = reqShmPtr.isPresent()
                    ? LINKER.downcallHandle(reqShmPtr.get(), FunctionDescriptor.of(ValueLayout.ADDRESS))
                    : null;

            var respShmPtr = lookup.find("rhb_get_resp_shm_ptr");
            rhbGetRespShmPtr = respShmPtr.isPresent()
                    ? LINKER.downcallHandle(respShmPtr.get(), FunctionDescriptor.of(ValueLayout.ADDRESS))
                    : null;

            loaded = true;
        } catch (Throwable e) {
            loadError = e.getMessage();
            System.err.println("[RustHttpBridge] rust_http_bridge 加载失败: " + e.getMessage());
        }
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
     * 校验原生库已加载。
     */
    private static void ensureLoaded() {
        if (!loaded) {
            throw new IllegalStateException("rust_http_bridge 未加载: " + (loadError == null ? "未知错误" : loadError));
        }
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
        ensureLoaded();
        // 分配原生内存，确保 rhb_start 内部任何线程都能安全访问（生命周期由 Arena 管理）
        var arena = Arena.ofAuto();
        MemorySegment nameSeg = arena.allocateFrom(shmName);
        int rc;
        try {
            rc = (int) rhbStart.invokeExact(port, nameSeg, capacity, slotSize);
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_start 异常: " + e.getMessage(), e);
        }
        if (rc != 0) {
            throw new IllegalStateException("rhb_start 失败, rc=" + rc);
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
        ensureLoaded();
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
        ensureLoaded();
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

    /** 获取请求队列通知 fd（Windows: Event Object Handle, Linux: eventfd）。启动后调用一次。 */
    public static int getReqNotifyFd() {
        ensureLoaded();
        if (rhbGetReqNotifyFd == null) return -1;
        try {
            return (int) rhbGetReqNotifyFd.invokeExact();
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_get_req_notify_fd 异常: " + e.getMessage(), e);
        }
    }

    /** 获取响应队列通知 fd。 */
    public static int getRespNotifyFd() {
        ensureLoaded();
        if (rhbGetRespNotifyFd == null) return -1;
        try {
            return (int) rhbGetRespNotifyFd.invokeExact();
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_get_resp_notify_fd 异常: " + e.getMessage(), e);
        }
    }

    /** 获取请求队列 SHM 基地址 MemorySegment（零 FFM 直接读写）。启动后调用一次，结果永久有效。 */
    public static MemorySegment getReqShmSegment() {
        ensureLoaded();
        if (rhbGetReqShmPtr == null) return null;
        try {
            return (MemorySegment) rhbGetReqShmPtr.invokeExact();
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_get_req_shm_ptr 异常: " + e.getMessage(), e);
        }
    }

    /** 获取响应队列 SHM 基地址 MemorySegment。 */
    public static MemorySegment getRespShmSegment() {
        ensureLoaded();
        if (rhbGetRespShmPtr == null) return null;
        try {
            return (MemorySegment) rhbGetRespShmPtr.invokeExact();
        } catch (Throwable e) {
            throw new IllegalStateException("rhb_get_resp_shm_ptr 异常: " + e.getMessage(), e);
        }
    }

    /** 等待请求队列通知（Windows: WaitForSingleObject, Linux: 轮询）。阻塞直到有新请求或超时。 */
    public static int waitForReq(int timeoutMs) {
        if (waitForSingleObject != null) {
            try {
                int fd = getReqNotifyFd();
                return (int) waitForSingleObject.invokeExact(MemorySegment.ofAddress(fd), timeoutMs);
            } catch (Throwable e) {
                throw new IllegalStateException("WaitForSingleObject 异常: " + e.getMessage(), e);
            }
        }
        return -1; // 非 Windows 平台回退到轮询
    }

    @Override
    public void close() {
        stop();
    }
}
