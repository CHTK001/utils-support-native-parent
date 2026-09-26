package com.chua.nativemetrics.support;

import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;
import lombok.extern.slf4j.Slf4j;

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
 * 指标 NAT 库（指标_NAT）Java 侧封装，基于 JDK Panama FFI 调用系统采样器。
 * <p>
 * 通过 {@link NativeLoader} 加载 {@code metrics_native} 库，提供 启动/poll/停止 三个原语以及 auto关闭 支持。
 * </p>
 *
 * <p>本类原位于 {@code utils-support-metrics-starter} 的
 * {@code com.chua.metrics.support} 包，为让 FFM 绑定与动态库同住而迁入本模块。
 * 调用点：{@code MetricsService}。</p>
 *
 * @author CH
 * @since 4.0.0
 */
@Slf4j
public class MetricsNativeLibrary implements AutoCloseable {

    /**
     * 系统 Linker
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 原生符号绑定的惰性持有者。
     *
     * <p>库加载与符号解析放在独立持有者里，而不是本类的静态块里，是为了保住
     * {@link #create()} 既有的「加载失败返回空实例」语义：类级 {@code <clinit>}
     * 一旦抛错，JVM 会把该类标记为初始化失败，后续任何访问都直接变成
     * {@code NoClassDefFoundError}，连捕获重试的机会都没有。此前 Linux 上原生
     * 绑定解析失败即如此，{@code create()} 的 try/catch 形同虚设，异常一路穿透
     * 到 Spring 上下文导致应用整体启动失败。</p>
     */
    private static final class Bindings {

        /**
         * 启动采样器方法句柄
         */
        private static final MethodHandle START_SAMPLER;

        /**
         * 停止采样器方法句柄
         */
        private static final MethodHandle STOP_SAMPLER;

        /**
         * 查询快照字节长度方法句柄
         */
        private static final MethodHandle SNAPSHOT_SIZE;

        /**
         * 拉取快照方法句柄
         */
        private static final MethodHandle GET_SNAPSHOT;

        static {
            try {
                Path target = NativeUtils.tempRoot().resolve("metrics-native");
                // 必须显式给 glob：NativeLoader 默认 glob 是 "*.dll"，只匹配 Windows，
                // Linux/macOS 的 libmetrics_native.{so,dylib} 会匹配失败
                NativeLoader.of("metrics_native").toTarget(target).glob("*metrics_native*").load();
                SymbolLookup lookup = SymbolLookup.loaderLookup();
                START_SAMPLER = LINKER.downcallHandle(
                        lookup.find("metrics_start_sampler").orElseThrow(),
                        FunctionDescriptor.ofVoid(ValueLayout.JAVA_LONG)
                );
                STOP_SAMPLER = LINKER.downcallHandle(
                        lookup.find("metrics_stop_sampler").orElseThrow(),
                        FunctionDescriptor.ofVoid()
                );
                SNAPSHOT_SIZE = LINKER.downcallHandle(
                        lookup.find("metrics_snapshot_size").orElseThrow(),
                        FunctionDescriptor.of(ValueLayout.JAVA_INT)
                );
                GET_SNAPSHOT = LINKER.downcallHandle(
                        lookup.find("metrics_get_snapshot").orElseThrow(),
                        FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT)
                );
            } catch (Throwable e) {
                throw new ExceptionInInitializerError(e);
            }
        }

        /**
         * 触发本持有者的静态初始化，用于探测原生绑定是否可用。
         *
         * <p>初始化失败时这里会抛出 {@code NoClassDefFoundError}，由
         * {@link #create()} 统一兜住；成功则为空操作。</p>
         */
        static void ensureAvailable() {
            // 静态初始化已完成即代表绑定可用
        }
    }

    /**
     * 采样器是否已启动
     */
    private volatile boolean started;

    /**
     * 尝试加载 NAT 库并返回实例；加载失败时返回 空 并记录错误日志。
     *
     * @return MetricsNativeLibrary 实例（失败时为 空）
     */
    public static MetricsNativeLibrary create() {
        try {
            Bindings.ensureAvailable();
            return new MetricsNativeLibrary();
        } catch (Throwable e) {
            log.error("无法加载 metrics_native native 库，指标采样降级为不可用", e);
            return null;
        }
    }

    /**
     * 私有构造函数。
     */
    private MetricsNativeLibrary() {
        this.started = false;
    }

    /**
     * 启动采样器。
     *
     * @param intervalMs 采样间隔（毫秒），必须大于 0
     * @throws IllegalArgumentException 当 间隔ms &lt;= 0
     */
    public void start(long intervalMs) {
        if (started) {
            return;
        }
        if (intervalMs <= 0) {
            throw new IllegalArgumentException("intervalMs 必须大于 0");
        }
        try {
            Bindings.START_SAMPLER.invokeExact(intervalMs);
            started = true;
        } catch (Throwable t) {
            throw new RuntimeException(t);
        }
    }

    /**
     * 拉取当前快照的 UTF-8 字符串表示。
     *
     * @return 快照内容；未启动或拉取失败返回 空
     */
    public String poll() {
        if (!started) {
            throw new IllegalStateException("采样器未启动，请先调用 start()");
        }
        try {
            int size = (int) Bindings.SNAPSHOT_SIZE.invokeExact();
            if (size <= 0) {
                return null;
            }
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment segment = arena.allocate((long) size);
                int copied = (int) Bindings.GET_SNAPSHOT.invokeExact(segment, size);
                if (copied != size) {
                    log.warn("Snapshot size mismatch: expected={}, got={}", size, copied);
                    return null;
                }
                byte[] bytes = segment.toArray(ValueLayout.JAVA_BYTE);
                return new String(bytes, StandardCharsets.UTF_8);
            }
        } catch (Throwable t) {
            throw new RuntimeException(t);
        }
    }

    /**
     * 停止采样器（幂等）。
     */
    public void stop() {
        if (started) {
            try {
                Bindings.STOP_SAMPLER.invokeExact();
                started = false;
            } catch (Throwable t) {
                throw new RuntimeException(t);
            }
        }
    }

    /**
     * 关闭资源，等价于 {@link #stop()}。
     */
    @Override
    public void close() {
        stop();
    }
}
