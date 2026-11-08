package com.chua.nativerdp.support;

import com.sun.jna.Memory;
import com.sun.jna.Native;
import com.sun.jna.Pointer;
import com.sun.jna.ptr.IntByReference;
import com.chua.common.support.utils.NativeLoader;
import com.chua.common.support.utils.NativeUtils;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.stream.Stream;

/**
 * RDP 远控桥接器（FreeRDP 3.x C 扩展 librdpj 的 JNA 绑定）
 * <p>
 * 动态库随 classpath 的 {@code native/{platformdir}/} 目录分发，按平台自动抽取到
 * {@code java.io.tmpdir/chua-native/chua_native_rdpj}，由
 * {@link NativeLoader} 负责拷贝；JNA 以抽取后的绝对路径建立接口句柄。
 * </p>
 * <p>
 * 加载失败被统一收敛到 {@link #loaded}（不阻断 JVM 启动），{@link #isLoaded()} 返回
 * {@code false} 时由调用方拒绝 RDP 连接；帧约定为 32 字节帧头（width/height/stride/
 * bpp/x/y/cx/cy）+ BGRA 像素。
 * </p>
 *
 * @author CH
 * @since 4.0.0.52
 */
public final class RdpjBridge {

    /** 动态库逻辑名（纯名称，不含 lib 前缀/后缀） */
    private static final String LIBRARY_NAME = "rdpj";

    /** 动态库在运行时是否已加载 */
    private static volatile boolean loaded;

    /** 加载失败时的根异常（成功时为 null，调用方据此拒绝 RDP 连接） */
    private static volatile Throwable loadError;

    private static volatile Rdpj rdpj;

    /**
     * JNA 调用界面（与 rdpj.h 符号一一对应）
     */
    public interface Rdpj extends com.sun.jna.Library {

        /**
         * 创建句柄
         *
         * @return 句柄（指针）
         */
        Pointer rdpj_create();

        /**
         * 销毁句柄
         *
         * @param handle 句柄
         * @return 0 成功，-1 非法
         */
        int rdpj_destroy(Pointer handle);

        /**
         * 发起连接（非阻塞，连接泵循环在专用线程中）
         *
         * @param handle 句柄
         * @param hostname 目标主机
         * @param port 端口（默认 3389）
         * @param username 用户名
         * @param password 密码
         * @param locale 语言区域（如 zh-CN）
         * @return 0 成功，-1 非法
         */
        int rdpj_connect(Pointer handle, String hostname, int port,
                         String username, String password, String locale);

        /**
         * 断开连接
         *
         * @param handle 句柄
         * @return 0 成功，-1 非法
         */
        int rdpj_close(Pointer handle);

        /**
         * 获取一帧（返回 0 = 无新帧，否则 = 写入 out 的字节数：32 字节帧头 + BGRA 像素）
         *
         * @param handle 句柄
         * @param out 输出缓冲区
         * @param outSize 可写入的缓冲区大小
         * @param outLen 写入长度（返回）
         * @return 写入的字节数，0 表示没有新帧
         */
        int rdpj_frame(Pointer handle, Pointer out, int outSize,
                       com.sun.jna.ptr.IntByReference outLen);

        /**
         * 查询最后一帧信息（不消费帧、不复制，只读）
         *
         * @param handle 句柄
         * @return 帧信息指针（空指针表示无数据）
         */
        Pointer rdpj_last_frame(Pointer handle);

        /**
         * 鼠标事件（button：1=左 2=右 4=中，down：1= 按下 0= 抬起，
         * x/y 为桌面（即帧的 width/height）坐标）
         *
         * @param handle 句柄
         * @param x X 坐标
         * @param y Y 坐标
         * @param button 按钮
         * @param down 是否按下
         * @return 0 成功，-1 非法
         */
        int rdpj_mouse(Pointer handle, int x, int y, int button, boolean down);

        /**
         * 键盘事件（keysym: RDP 扫描码）
         *
         * @param handle 句柄
         * @param keysym RDP 扫描码
         * @param down 是否按下
         * @return 0 成功，-1 非法
         */
        int rdpj_key(Pointer handle, short keysym, boolean down);

        /**
         * 切换桌面（桌面大小变化时）
         *
         * @param handle 句柄
         * @param width 宽
         * @param height 高
         * @return 0 成功，-1 非法
         */
        int rdpj_resize(Pointer handle, int width, int height);

        /**
         * 状态信息
         *
         * @param handle 句柄
         * @return 状态字符串
         */
        String rdpj_status(Pointer handle);

        /**
         * 错误码
         *
         * @param handle 句柄
         * @return 错误码
         */
        int rdpj_errno(Pointer handle);
    }

    private RdpjBridge() {
    }

    /**
     * 原生库是否已加载
     *
     * @return true 表示已加载
     */
    public static boolean isLoaded() {
        return loaded;
    }

    /**
     * 加载时的错误（如库文件缺失）
     *
     * @return 错误，加载成功时为 {@code null}
     */
    public static Throwable loadError() {
        return loadError;
    }

    /**
     * 获取 JNA 接口（延迟加载，调用方在握手时检查 {@link #isLoaded()}）
     *
     * @return 接口句柄
     */
    public static Rdpj instance() {
        if (!loaded) {
            load();
        }
        return rdpj;
    }

    /**
     * 加载 RDP 原生库（失败时不抛异常，由 {@link #isLoaded()} 看护）
     */
    public static synchronized void load() {
        if (loaded) {
            return;
        }
        try {
            final Path target = NativeUtils.tempRoot().resolve("chua_native_rdpj");
            NativeLoader.of("chua_native_rdpj")
                    .glob("*" + LIBRARY_NAME + "*")
                    .toTarget(target)
                    .load();

            // 查找实际提取出的动态库文件（linux: librdpj.so, windows: rdpj.dll,
            // darwin: librdpj.dylib），由 JNA 建立接口；已经 System.load() 过，
            // Native.load 只取回已有句柄
            Path libraryPath = null;
            try (Stream<Path> walk = Files.list(target)) {
                libraryPath = walk.filter(Files::isRegularFile)
                        .filter(p -> p.getFileName().toString()
                                .matches("(?i).*" + LIBRARY_NAME + "\\.(so|dll|dylib).*"))
                        .findFirst()
                        .orElse(target.resolve("librdpj.so"));  // 兜底名称
            }
            rdpj = Native.load(libraryPath.toString(), Rdpj.class);
            loaded = true;
        } catch (Throwable e) {
            loadError = e;
            loaded = false;
        }
    }
}
