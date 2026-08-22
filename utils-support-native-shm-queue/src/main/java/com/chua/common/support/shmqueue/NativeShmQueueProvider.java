package com.chua.common.support.shmqueue;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Objects;

/**
 * libshmqueue 的 Panama FFM 绑定实现。
 *
 * <p>运行时通过 {@link NativeLoader} 将 {@code shmqueue.dll/.so/.dylib} 从
 * classpath:/native/{platformDir}/ 抽取到临时目录并加载，
 * 再以 {@link Linker} 绑定 C 侧 {@code shmq_*} 符号。</p>
 *
 * <p>本类为 {@link ShmQueueProvider} 的 SPI 实现，注册于
 * {@code META-INF/services/com.chua.common.support.shmqueue.ShmQueueProvider}。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public class NativeShmQueueProvider implements ShmQueueProvider {

    /** 原生链接器 */
    private static final Linker LINKER = Linker.nativeLinker();

    /** 是否已加载成功 */
    private static volatile boolean loaded = false;

    /** 加载失败信息（诊断用） */
    private static volatile String loadError;

    /** shmq_create 句柄 */
    private static MethodHandle shmqCreate;

    /** shmq_attach 句柄 */
    private static MethodHandle shmqAttach;

    /** shmq_send 句柄 */
    private static MethodHandle shmqSend;

    /** shmq_recv 句柄 */
    private static MethodHandle shmqRecv;

    /** shmq_recv_timeout 句柄 */
    private static MethodHandle shmqRecvTimeout;

    /** shmq_set_spin_ns 句柄 */
    private static MethodHandle shmqSetSpinNs;

    /** shmq_capacity 句柄 */
    private static MethodHandle shmqCapacity;

    /** shmq_slot_size 句柄 */
    private static MethodHandle shmqSlotSize;

    /** shmq_mode 句柄 */
    private static MethodHandle shmqMode;

    /** shmq_destroy 句柄 */
    private static MethodHandle shmqDestroy;

    static {
        try {
            String libName = System.mapLibraryName("shmqueue");
            Path nativeDir = NativeUtils.tempRoot().resolve("shmqueue");
            NativeLoader.of("shmqueue")
                    .toTarget(nativeDir)
                    .glob("*shmqueue*")
                    .load();
            Path libPath = nativeDir.resolve(libName);
            if (!Files.exists(libPath)) {
                throw new IllegalStateException("未找到原生库: " + libPath);
            }
            SymbolLookup lookup = SymbolLookup.libraryLookup(libPath, Arena.ofAuto());

            FunctionDescriptor ctxArgs = FunctionDescriptor.of(ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS);
            shmqCreate = LINKER.downcallHandle(lookup.find("shmq_create").orElseThrow(), ctxArgs);
            shmqAttach = LINKER.downcallHandle(lookup.find("shmq_attach").orElseThrow(), ctxArgs);

            FunctionDescriptor sendDesc = FunctionDescriptor.of(ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT);
            shmqSend = LINKER.downcallHandle(lookup.find("shmq_send").orElseThrow(), sendDesc);

            FunctionDescriptor recvArgs = FunctionDescriptor.of(ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS);
            shmqRecv = LINKER.downcallHandle(lookup.find("shmq_recv").orElseThrow(), recvArgs);

            FunctionDescriptor recvTimeoutArgs = FunctionDescriptor.of(ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG);
            shmqRecvTimeout = LINKER.downcallHandle(lookup.find("shmq_recv_timeout").orElseThrow(), recvTimeoutArgs);

            shmqSetSpinNs = LINKER.downcallHandle(lookup.find("shmq_set_spin_ns").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

            FunctionDescriptor getterArgs = FunctionDescriptor.of(ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS);
            shmqCapacity = LINKER.downcallHandle(lookup.find("shmq_capacity").orElseThrow(), getterArgs);
            shmqSlotSize = LINKER.downcallHandle(lookup.find("shmq_slot_size").orElseThrow(), getterArgs);
            shmqMode = LINKER.downcallHandle(lookup.find("shmq_mode").orElseThrow(), getterArgs);

            shmqDestroy = LINKER.downcallHandle(lookup.find("shmq_destroy").orElseThrow(),
                    FunctionDescriptor.ofVoid(ValueLayout.ADDRESS, ValueLayout.JAVA_INT));

            loaded = true;
        } catch (Throwable e) {
            loadError = e.getMessage();
            System.err.println("[NativeShmQueueProvider] libshmqueue 加载失败: " + e.getMessage());
        }
    }

    /**
     * 校验原生库已加载
     */
    private static void ensureLoaded() {
        if (!loaded) {
            throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG,
                    "libshmqueue 未加载: " + (loadError == null ? "未知错误" : loadError));
        }
    }

    @Override
    public ShmQueue create(String name, int capacity, int slotSize, ShmQueue.Mode mode) {
        ensureLoaded();
        Objects.requireNonNull(name, "name");
        if (capacity < 2) {
            throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG, "capacity 必须 >= 2");
        }
        if (slotSize < 16) {
            throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG, "slotSize 必须 >= 16");
        }
        MemorySegment ctx = createOrAttach(shmqCreate, name, capacity, slotSize, mode.code);
        if (ctx == null) {
            throw new ShmQueueException(ShmQueue.ERR_OPEN_SHM, "创建共享内存队列失败: " + name);
        }
        return new NativeShmQueue(ctx);
    }

    @Override
    public ShmQueue attach(String name) {
        ensureLoaded();
        Objects.requireNonNull(name, "name");
        MemorySegment ctx = createOrAttach(shmqAttach, name, 0, 0, ShmQueue.Mode.HYBRID.code);
        if (ctx == null) {
            throw new ShmQueueException(ShmQueue.ERR_OPEN_SHM, "attach 共享内存队列失败: " + name);
        }
        return new NativeShmQueue(ctx);
    }

    /**
     * 调用 shmq_create / shmq_attach 并返回队列上下文地址
     *
     * @param handle  方法句柄
     * @param name    队列名
     * @param capacity 容量（attach 传 0）
     * @param slotSize 槽大小（attach 传 0）
     * @param mode    模式
     * @return 队列上下文，失败返回 null
     */
    private static MemorySegment createOrAttach(MethodHandle handle, String name,
                                                int capacity, int slotSize, int mode) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment nameSeg = arena.allocateFrom(name);
            MemorySegment ctxOut = arena.allocate(ValueLayout.ADDRESS);
            int rc = (int) handle.invokeExact(nameSeg, (long) 0, capacity, slotSize, mode, ctxOut);
            if (rc != 0) {
                return null;
            }
            MemorySegment ctxSeg = ctxOut.get(ValueLayout.ADDRESS, 0);
            if (ctxSeg == null || ctxSeg.equals(MemorySegment.NULL)) {
                return null;
            }
            // 返回值需跨 arena 存活，用 raw 地址重新包装为无作用域约束的段
            return MemorySegment.ofAddress(ctxSeg.address());
        } catch (Throwable e) {
            throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG, "调用原生队列失败: " + e.getMessage());
        }
    }

    /**
     * 具体队列实现：持有 C 上下文地址，封装全部收发操作。
     */
    private static final class NativeShmQueue extends ShmQueue {

        /** C 侧上下文地址 */
        private MemorySegment ctx;

        /** 是否已关闭 */
        private volatile boolean closed;

        /** 槽大小（由 C 侧读取，用于预分配接收缓冲） */
        private int slotSize;

        NativeShmQueue(MemorySegment ctx) {
            this.ctx = ctx;
            this.slotSize = readInt(shmqSlotSize, -1);
        }

        @Override
        public void send(int msgType, byte[] data) {
            checkOpen();
            byte[] payload = data == null ? new byte[0] : data;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment dataSeg = arena.allocate(payload.length);
                MemorySegment.copy(payload, 0, dataSeg, ValueLayout.JAVA_BYTE, 0, payload.length);
                int rc = (int) shmqSend.invokeExact(ctx, msgType, dataSeg, payload.length);
                if (rc != 0) {
                    throw new ShmQueueException(rc, "发送失败: " + errorMessage(rc));
                }
            } catch (Throwable e) {
                if (e instanceof ShmQueueException sqe) {
                    throw sqe;
                }
                throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG, "发送异常: " + e.getMessage());
            }
        }

        @Override
        public Message recv() {
            return doRecv(0L);
        }

        @Override
        public Message recvTimeout(long timeoutNanos) {
            return doRecv(timeoutNanos);
        }

        /**
         * 执行接收
         *
         * @param timeoutNanos 超时纳秒，&lt;=0 表示无限等待
         * @return 消息
         */
        private Message doRecv(long timeoutNanos) {
            checkOpen();
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment buf = arena.allocate(Math.max(16, slotSize));
                MemorySegment msgType = arena.allocate(ValueLayout.JAVA_INT);
                MemorySegment lenOut = arena.allocate(ValueLayout.JAVA_INT);
                int rc;
                if (timeoutNanos <= 0) {
                    rc = (int) shmqRecv.invokeExact(ctx, msgType, buf, slotSize, lenOut);
                } else {
                    rc = (int) shmqRecvTimeout.invokeExact(ctx, msgType, buf, slotSize, lenOut, timeoutNanos);
                }
                if (rc != 0) {
                    throw new ShmQueueException(rc, "接收失败: " + errorMessage(rc));
                }
                int mt = msgType.get(ValueLayout.JAVA_INT, 0);
                int len = lenOut.get(ValueLayout.JAVA_INT, 0);
                byte[] bytes = new byte[len];
                MemorySegment.copy(buf, ValueLayout.JAVA_BYTE, 0, bytes, 0, len);
                return new Message(mt, bytes);
            } catch (Throwable e) {
                if (e instanceof ShmQueueException sqe) {
                    throw sqe;
                }
                throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG, "接收异常: " + e.getMessage());
            }
        }

        @Override
        public void setSpinNanos(long spinNs) {
            checkOpen();
            try {
                int rc = (int) shmqSetSpinNs.invokeExact(ctx, spinNs);
                if (rc != 0) {
                    throw new ShmQueueException(rc, "设置自旋时间失败: " + errorMessage(rc));
                }
            } catch (Throwable e) {
                if (e instanceof ShmQueueException sqe) {
                    throw sqe;
                }
                throw new ShmQueueException(ShmQueue.ERR_INVALID_ARG, "设置自旋时间异常: " + e.getMessage());
            }
        }

        /**
         * 读取 C 侧 int 属性
         *
         * @param handle 方法句柄
         * @param def    失败默认值
         * @return 属性值
         */
        private int readInt(MethodHandle handle, int def) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.JAVA_INT);
                int rc = (int) handle.invokeExact(ctx, out);
                if (rc != 0) {
                    return def;
                }
                return out.get(ValueLayout.JAVA_INT, 0);
            } catch (Throwable e) {
                return def;
            }
        }

        /**
         * 校验未关闭
         */
        private void checkOpen() {
            if (closed || ctx == null) {
                throw new ShmQueueException(ShmQueue.ERR_DESTROYED, "队列已关闭");
            }
        }

        /**
         * 错误码转描述
         *
         * @param rc C 端错误码
         * @return 描述
         */
        private static String errorMessage(int rc) {
            return switch (rc) {
                case ShmQueue.ERR_INVALID_ARG -> "invalid argument";
                case ShmQueue.ERR_NOMEM -> "out of memory";
                case ShmQueue.ERR_OPEN_SHM -> "open shm failed";
                case ShmQueue.ERR_TRUNCATE -> "truncate failed";
                case ShmQueue.ERR_MMAP -> "mmap failed";
                case ShmQueue.ERR_HEADER_MAGIC -> "header magic mismatch";
                case ShmQueue.ERR_HEADER_VERSION -> "header version mismatch";
                case ShmQueue.ERR_QUEUE_FULL -> "queue full";
                case ShmQueue.ERR_DATA_TOO_LARGE -> "data too large for slot";
                case ShmQueue.ERR_WRITE_FD -> "write notify fd failed";
                case ShmQueue.ERR_READ_FD -> "read notify fd failed";
                case ShmQueue.ERR_TIMEOUT -> "recv timeout";
                case ShmQueue.ERR_NOT_SUPPORTED -> "not supported on this platform";
                case ShmQueue.ERR_DESTROYED -> "queue destroyed";
                default -> "error(" + rc + ")";
            };
        }

        @Override
        public void close() {
            if (closed) {
                return;
            }
            closed = true;
            if (ctx != null) {
                try {
                    shmqDestroy.invokeExact(ctx, 0);
                } catch (Throwable ignored) {
                    // 关闭失败忽略
                }
                ctx = null;
            }
        }
    }
}
