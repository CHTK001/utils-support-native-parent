package com.chua.common.support.nativehttp.server;

import com.chua.common.support.nativehttp.RustHttpBridge;
import com.chua.common.support.network.ProtocolType;
import com.chua.common.support.network.server.AbstractServer;
import com.chua.common.support.network.server.ServerSetting;
import com.chua.common.support.spi.annotations.Spi;
import lombok.extern.slf4j.Slf4j;

import java.nio.charset.StandardCharsets;

/**
 * 基于共享内存 + Rust hyper 的 HTTP 服务器。
 *
 * <p>Java 是 HTTP 入口：本类负责启动 Rust 动态库（{@link RustHttpBridge}），
 * 并运行一个轮询线程从请求队列取出请求，交给框架过滤器链处理，
 * 最后将响应写回响应队列，由 Rust hyper 完成真正的网络回包。</p>
 *
 * <h2>请求信封</h2>
 * <pre>
 * req_id(8,u64 LE) | method_len(2,u16 LE) | path_len(2,u16 LE)
 * | body_len(4,u32 LE) | method | path | body
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
@Spi({"shm-http"})
public class ShmHttpServer extends AbstractServer {

    /** 默认队列容量 */
    private static final int DEFAULT_CAPACITY = 1024;

    /** 默认槽大小（字节），需能容纳最大请求/响应信封 */
    private static final int DEFAULT_SLOT_SIZE = 16 * 1024;

    /** 轮询空转等待（毫秒） */
    private static final long POLL_IDLE_SLEEP_MS = 5;

    /** HTTP 桥接 */
    private RustHttpBridge bridge;

    /** 轮询线程 */
    private Thread pollThread;

    /** 队列容量 */
    private final int capacity;

    /** 槽大小 */
    private final int slotSize;

    /**
     * 创建 ShmHttpServer 实例
     *
     * @param setting setting
     */
    public ShmHttpServer(ServerSetting setting) {
        this(setting, DEFAULT_CAPACITY, DEFAULT_SLOT_SIZE);
    }

    /**
     * 创建 ShmHttpServer 实例
     *
     * @param setting  setting
     * @param capacity 队列容量
     * @param slotSize 槽大小
     */
    public ShmHttpServer(ServerSetting setting, int capacity, int slotSize) {
        super(setting);
        this.capacity = capacity;
        this.slotSize = slotSize;
    }

    @Override
    protected void doStart() {
        String shmName = "rhb_" + setting.getPort();
        bridge = RustHttpBridge.start(setting.getPort(), shmName, capacity, slotSize);
        pollThread = new Thread(this::pollLoop, "shm-http-poll");
        pollThread.setDaemon(true);
        pollThread.start();
        log.info("ShmHttpServer 桥接启动: port={}, shm={}, capacity={}, slotSize={}",
                setting.getPort(), shmName, capacity, slotSize);
    }

    @Override
    protected void doStop() {
        running = false;
        if (pollThread != null) {
            try { pollThread.join(3000); }
            catch (InterruptedException e) { Thread.currentThread().interrupt(); }
            pollThread = null;
        }
        if (bridge != null) {
            bridge.stop();
            bridge = null;
        }
    }

    @Override
    /** Do停止Accepting */
    protected void doStopAccepting() {
        running = false;
    }

    @Override
    /** 获取ProtocolType */
    public ProtocolType getProtocolType() {
        return ProtocolType.HTTP;
    }

    /**
     * 轮询请求队列并分发。
     *
     * <p>轮询线程（req 队列唯一消费者，保持 SPSC）只做：取信封 → 解析 → 异步提交。
     * 过滤器链在 AbstractServer 内部虚拟线程上并行执行，无需自行管理线程池。</p>
     */
    private void pollLoop() {
        var buf = new byte[slotSize];
        while (running) {
            int n;
            try {
                n = bridge.pollRequest(buf);
                if (n > 0) {
                    var data = parseEnvelope(buf, n);
                    if (data != null) {
                        dispatch(data);
                    }
                } else {
                    for (int i = 0; i < 10; i++) Thread.onSpinWait();
                }
            } catch (Exception e) {
                log.debug("轮询请求队列异常: {}", e.getMessage());
                try { Thread.sleep(10); } catch (InterruptedException ie) { return; }
            }
        }
    }

    /**
     * 解析请求信封
     *
     * @param buf 信封缓冲
     * @param n   信封长度
     * @return 解析结果，非法信封返回 null
     */
    private RequestData parseEnvelope(byte[] buf, int n) {
        if (n < 16) {
            return null;
        }
        long reqId = readLeU64(buf, 0);
        int methodLen = readLeU16(buf, 8);
        int pathLen = readLeU16(buf, 10);
        int bodyLen = readLeU32(buf, 12);
        int total = 16 + methodLen + pathLen + bodyLen;
        if (total > n) {
            return null;
        }
        String method = new String(buf, 16, methodLen, StandardCharsets.UTF_8);
        String path = new String(buf, 16 + methodLen, pathLen, StandardCharsets.UTF_8);
        byte[] body = new byte[bodyLen];
        if (bodyLen > 0) {
            System.arraycopy(buf, 16 + methodLen + pathLen, body, 0, bodyLen);
        }
        return new RequestData(reqId, method, path, body);
    }

    /**
     * 在 worker 线程上执行分发：过滤器链处理 + 回写响应。
     *
     * @param data 请求数据
     */
    private void dispatch(RequestData data) {
        ShmServerRequest request = new ShmServerRequest(data.method, data.path, data.body);
        ShmServerResponse response = new ShmServerResponse();
        try {
            handleRequestWithStage(request, response).whenComplete((v, ex) -> {
                byte[] respBody = response.getBody() != null ? response.getBody() : new byte[0];
                String ct = response.getContentType();
                try {
                    bridge.sendResponse(data.reqId, response.getStatus(), ct, respBody);
                } catch (Throwable t) {
                    log.warn("SHM 响应回写失败 req_id={}: {}", data.reqId, t.getMessage());
                }
            });
        } catch (Throwable e) {
            log.warn("请求处理异常 req_id={}: {}", data.reqId, e.getMessage());
            byte[] err = e.getMessage() == null
                    ? "Internal Server Error".getBytes(StandardCharsets.UTF_8)
                    : e.getMessage().getBytes(StandardCharsets.UTF_8);
            try {
                bridge.sendResponse(data.reqId, 500, "text/plain; charset=utf-8", err);
            } catch (Throwable t) {
                log.warn("SHM sendResponse(500) 失败 req_id={}: {}", data.reqId, t.getMessage());
            }
        }
    }

    /**
     * 请求信封解析结果（不可变，可安全跨线程传递）
     */
    private record RequestData(long reqId, String method, String path, byte[] body) {
    }

    /** 读取 u64 LE */
    private static long readLeU64(byte[] b, int off) {
        return (b[off] & 0xFFL)
                | ((b[off + 1] & 0xFFL) << 8)
                | ((b[off + 2] & 0xFFL) << 16)
                | ((b[off + 3] & 0xFFL) << 24)
                | ((b[off + 4] & 0xFFL) << 32)
                | ((b[off + 5] & 0xFFL) << 40)
                | ((b[off + 6] & 0xFFL) << 48)
                | ((b[off + 7] & 0xFFL) << 56);
    }

    /** 读取 u16 LE */
    private static int readLeU16(byte[] b, int off) {
        return (b[off] & 0xFF) | ((b[off + 1] & 0xFF) << 8);
    }

    /** 读取 u32 LE */
    private static int readLeU32(byte[] b, int off) {
        return (b[off] & 0xFF)
                | ((b[off + 1] & 0xFF) << 8)
                | ((b[off + 2] & 0xFF) << 16)
                | ((b[off + 3] & 0xFF) << 24);
    }
}
