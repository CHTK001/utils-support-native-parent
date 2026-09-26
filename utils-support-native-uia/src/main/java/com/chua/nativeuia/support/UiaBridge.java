package com.chua.nativeuia.support;

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
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Windows UI Automation 通用原生库 FFM 桥接器。
 *
 * <p>基于 Java 25 FFM（Foreign Function &amp; Memory）API 绑定本模块随 jar 分发的
 * 自研动态库（{@code uia_rust.dll}，Rust + {@code IUIAutomation} COM 绑定，零第三方运行时依赖）。
 * 绑定的 C ABI 与 Rust 侧 {@code src/main/rust/src/lib.rs} 完全对齐：</p>
 * <ul>
 *   <li>{@code int uia_create(int64* out)}</li>
 *   <li>{@code int uia_destroy(int64 h)}</li>
 *   <li>{@code int uia_attach_window(int64 h, const char* sub, int visible, int64* hwnd)}</li>
 *   <li>{@code int uia_detach_window(int64 h)}</li>
 *   <li>{@code int uia_is_window_alive(int64 h, int64 hwnd, int* out)}</li>
 *   <li>{@code int uia_find(int64 h, const char* sel, int64* ids, int max, int* count)}</li>
 *   <li>{@code int uia_describe(int64 h, const char* idsJson, void** out)}</li>
 *   <li>{@code int uia_get_property(int64 h, int64 id, const char* prop, void** out)}</li>
 *   <li>{@code int uia_get_rect(int64 h, int64 id, int* out)}</li>
 *   <li>{@code int uia_set_value(int64 h, int64 id, const char* text, int* ok)}</li>
 *   <li>{@code int uia_invoke(int64 h, int64 id, int* ok)}</li>
 *   <li>{@code int uia_set_focus(int64 h, int64 id, int* ok)}</li>
 *   <li>{@code int uia_activate_window(int64 h, int* ok)}</li>
 *   <li>{@code int uia_click(int64 h, int64 id, int* ok)}</li>
 *   <li>{@code int uia_release(int64 h, int64 id)} / {@code int uia_release_all(int64 h)}</li>
 *   <li>{@code int uia_dump_tree(int64 h, int depth, int nodes, void** out)}</li>
 *   <li>{@code int uia_send_keys(const char* keys, int* ok)}</li>
 *   <li>{@code int uia_set_clipboard(const char* text, int* ok)}</li>
 *   <li>{@code void uia_free_string(void* p)}</li>
 *   <li>{@code const char* uia_last_error()}</li>
 * </ul>
 *
 * <h3>设计边界</h3>
 * <p>本库只提供<b>通用 UIA 原语</b>（窗口定位、控件树遍历、属性读取、模式操作、键鼠输入），
 * <b>不包含任何 IM 业务语义</b>。会话、消息、收信人等概念由上层模块通过
 * {@link UiaSelector}（JSON 选择器）描述控件结构后自行组装。</p>
 *
 * <h3>线程模型</h3>
 * <p>{@code uia_create} 内部调用 {@code CoInitializeEx(STA)}，COM 单元与线程绑定，
 * 因此本桥接器实例<b>必须与创建它的线程同生命周期，不可跨线程使用</b>。
 * 若需要多线程，请为每个线程各建一个实例。</p>
 *
 * <h3>平台支持</h3>
 * <p>UIA 为 Windows 独有技术，库文件随 jar 打包在 {@code /native/windows-*} 下，
 * 当前仅提供 {@code windows-x86_64}。调用 {@link #load()} 时自动从 classpath 抽取并加载。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class UiaBridge implements AutoCloseable {

    /**
     * 原生函数返回成功
     */
    public static final int RC_OK = 0;

    /**
     * 参数错误返回码
     */
    public static final int RC_ARG = 1;

    /**
     * COM / UIA 调用失败返回码
     */
    public static final int RC_FAIL = 2;

    /**
     * 句柄无效返回码
     */
    public static final int RC_INVALID = 3;

    /**
     * 原生库逻辑名（无平台前缀/后缀）
     */
    private static final String LIBRARY_NAME = "uia_rust";

    /**
     * 原生下行调用链接器
     */
    private static final Linker LINKER = Linker.nativeLinker();

    /**
     * 桥接器持有的共享内存会话（动态库与符号生命周期）
     */
    private final Arena arena;

    /**
     * 原生符号查找表
     */
    @SuppressWarnings("unused")
    private final SymbolLookup lookup;

    /**
     * 上下文句柄，0 表示未创建成功
     */
    private final long context;

    /**
     * 上下文是否已创建
     */
    private final boolean created;

    /**
     * uia_create 函数句柄
     */
    private final MethodHandle createHandle;

    /**
     * uia_destroy 函数句柄
     */
    private final MethodHandle destroyHandle;

    /**
     * uia_attach_window 函数句柄
     */
    private final MethodHandle attachWindowHandle;

    /**
     * uia_detach_window 函数句柄
     */
    private final MethodHandle detachWindowHandle;

    /**
     * uia_is_window_alive 函数句柄
     */
    private final MethodHandle isWindowAliveHandle;

    /**
     * uia_find 函数句柄
     */
    private final MethodHandle findHandle;

    /**
     * uia_describe 函数句柄
     */
    private final MethodHandle describeHandle;

    /**
     * uia_get_property 函数句柄
     */
    private final MethodHandle getPropertyHandle;

    /**
     * uia_get_rect 函数句柄
     */
    private final MethodHandle getRectHandle;

    /**
     * uia_set_value 函数句柄
     */
    private final MethodHandle setValueHandle;

    /**
     * uia_invoke 函数句柄
     */
    private final MethodHandle invokeHandle;

    /**
     * uia_set_focus 函数句柄
     */
    private final MethodHandle setFocusHandle;

    /**
     * uia_activate_window 函数句柄
     */
    private final MethodHandle activateWindowHandle;

    /**
     * uia_click 函数句柄
     */
    private final MethodHandle clickHandle;

    /**
     * uia_release 函数句柄
     */
    private final MethodHandle releaseHandle;

    /**
     * uia_release_all 函数句柄
     */
    private final MethodHandle releaseAllHandle;

    /**
     * uia_dump_tree 函数句柄
     */
    private final MethodHandle dumpTreeHandle;

    /**
     * uia_send_keys 函数句柄
     */
    private final MethodHandle sendKeysHandle;

    /**
     * uia_set_clipboard 函数句柄
     */
    private final MethodHandle setClipboardHandle;

    /**
     * uia_free_string 函数句柄
     */
    private final MethodHandle freeStringHandle;

    /**
     * uia_last_error 函数句柄
     */
    private final MethodHandle lastErrorHandle;

    /**
     * 判断当前平台是否有预编译动态库。
     *
     * @return 支持返回 true
     */
    public static boolean isSupported() {
        return "windows".equals(NativeUtils.getOsPrefixName());
    }

    /**
     * 从 classpath 抽取并加载自研动态库，创建 UIA 上下文。
     *
     * <p>库文件位于 {@code /native/{platform}/}，通过 {@link NativeLoader} 按平台抽取到临时目录后
     * 以 {@link SymbolLookup#libraryLookup(Path, Arena)} 加载，随后绑定全部 {@code uia_*} 导出符号
     * 并调用 {@code uia_create} 建立 COM 上下文。</p>
     *
     * @return 桥接器实例；上下文创建失败时抛出异常
     * @throws IllegalStateException 平台不支持、库缺失或符号绑定失败时抛出
     */
    public static UiaBridge load() {
        if (!isSupported()) {
            throw new IllegalStateException("UIA 库仅支持 Windows 平台，当前平台: "
                    + NativeUtils.getOsPrefixName());
        }
        String libFileName = NativeUtils.getLibraryFileName(LIBRARY_NAME, true);
        Path targetDir = NativeUtils.tempRoot().resolve(LIBRARY_NAME);

        // 从 classpath 抽取本平台动态库（仅抽取，由 FFM libraryLookup 自行加载）
        NativeLoader.of(LIBRARY_NAME)
                .toTarget(targetDir)
                .glob(libFileName)
                .extractOnly(true)
                .load();

        Path libFile = targetDir.resolve(libFileName);
        if (!Files.isRegularFile(libFile)) {
            throw new IllegalStateException("抽取 " + LIBRARY_NAME + " 动态库失败: " + libFile.toAbsolutePath());
        }

        Arena sharedArena = Arena.ofShared();
        try {
            SymbolLookup symbolLookup = SymbolLookup.libraryLookup(libFile, sharedArena);
            UiaBridge bridge = new UiaBridge(sharedArena, symbolLookup);
            log.info("UIA 自研原生库加载成功: {}", libFile.toAbsolutePath());
            return bridge;
        } catch (RuntimeException | Error e) {
            sharedArena.close();
            throw new IllegalStateException("加载 " + LIBRARY_NAME + " 动态库失败: " + e.getMessage(), e);
        }
    }

    /**
     * 私有构造器，绑定全部原生函数句柄并创建 COM 上下文。
     *
     * @param arena  共享内存会话
     * @param lookup 原生符号查找表
     */
    private UiaBridge(Arena arena, SymbolLookup lookup) {
        this.arena = arena;
        this.lookup = lookup;
        this.createHandle = bind(lookup, "uia_create",
                ValueLayout.JAVA_INT, ValueLayout.ADDRESS);
        this.destroyHandle = bind(lookup, "uia_destroy",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG);
        this.attachWindowHandle = bind(lookup, "uia_attach_window",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.ADDRESS);
        this.detachWindowHandle = bind(lookup, "uia_detach_window",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG);
        this.isWindowAliveHandle = bind(lookup, "uia_is_window_alive",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.findHandle = bind(lookup, "uia_find",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                ValueLayout.JAVA_INT, ValueLayout.ADDRESS);
        this.describeHandle = bind(lookup, "uia_describe",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.getPropertyHandle = bind(lookup, "uia_get_property",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.getRectHandle = bind(lookup, "uia_get_rect",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.setValueHandle = bind(lookup, "uia_set_value",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.invokeHandle = bind(lookup, "uia_invoke",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.setFocusHandle = bind(lookup, "uia_set_focus",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.activateWindowHandle = bind(lookup, "uia_activate_window",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.clickHandle = bind(lookup, "uia_click",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        this.releaseHandle = bind(lookup, "uia_release",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG);
        this.releaseAllHandle = bind(lookup, "uia_release_all",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG);
        this.dumpTreeHandle = bind(lookup, "uia_dump_tree",
                ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                ValueLayout.JAVA_INT, ValueLayout.ADDRESS);
        this.sendKeysHandle = bind(lookup, "uia_send_keys",
                ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.setClipboardHandle = bind(lookup, "uia_set_clipboard",
                ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        this.freeStringHandle = bindVoid(lookup, "uia_free_string", ValueLayout.ADDRESS);
        this.lastErrorHandle = bind(lookup, "uia_last_error", ValueLayout.ADDRESS);

        long ctx = 0L;
        boolean ok = false;
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.JAVA_LONG);
            out.set(ValueLayout.JAVA_LONG, 0L, 0L);
            int rc = (int) createHandle.invokeExact(out);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_create 失败, code=" + rc + ": " + lastError());
            }
            ctx = out.get(ValueLayout.JAVA_LONG, 0L);
            ok = ctx != 0L;
            if (!ok) {
                throw new IllegalStateException("uia_create 返回了空上下文句柄");
            }
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_create 调用异常: " + t.getMessage(), t);
        }
        this.context = ctx;
        this.created = ok;
    }

    /**
     * 获取底层上下文句柄。
     *
     * @return 上下文句柄
     */
    public long context() {
        return context;
    }

    /**
     * 判断上下文是否可用。
     *
     * @return 可用返回 true
     */
    public boolean isCreated() {
        return created;
    }

    /**
     * 按标题子串与可选类名绑定目标窗口，后续所有操作以该窗口为遍历根。
     *
     * <p>同一进程常有多个同名窗口（微信的主聊天窗口、内置文章浏览器、登录窗都会命中
     * "微信"）。原生层会在所有命中项中优先选择<b>面积最大</b>的窗口——
     * 主聊天窗口通常是该进程最大的窗口，附属窗口明显更小。</p>
     *
     * @param titleSubstring 窗口标题子串（大小写不敏感）
     * @param classSubstring 窗口类名子串（大小写不敏感），null 或空表示不限制
     * @param requireVisible  是否要求窗口可见
     * @return 绑定到的窗口句柄
     * @throws IllegalStateException 未找到窗口时抛出
     */
    public long attachWindow(String titleSubstring, String classSubstring, boolean requireVisible) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment needle = confined.allocateFrom(titleSubstring, StandardCharsets.UTF_8);
            MemorySegment classSeg = classSubstring == null || classSubstring.isBlank()
                    ? MemorySegment.NULL
                    : confined.allocateFrom(classSubstring, StandardCharsets.UTF_8);
            MemorySegment outHwnd = confined.allocate(ValueLayout.JAVA_LONG);
            outHwnd.set(ValueLayout.JAVA_LONG, 0L, 0L);
            int rc = (int) attachWindowHandle.invokeExact(context, needle, classSeg,
                    requireVisible ? 1 : 0, outHwnd);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_attach_window(" + titleSubstring
                        + ") 失败, code=" + rc + ": " + lastError());
            }
            return outHwnd.get(ValueLayout.JAVA_LONG, 0L);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_attach_window 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 按标题子串绑定目标窗口，不限制窗口类名。
     *
     * @param titleSubstring 窗口标题子串（大小写不敏感）
     * @param requireVisible 是否要求窗口可见
     * @return 绑定到的窗口句柄
     * @throws IllegalStateException 未找到窗口时抛出
     */
    public long attachWindow(String titleSubstring, boolean requireVisible) {
        return attachWindow(titleSubstring, null, requireVisible);
    }

    /**
     * 解绑目标窗口，后续操作回到桌面根元素。
     */
    public void detachWindow() {
        invokeVoid(detachWindowHandle, "uia_detach_window", context);
    }

    /**
     * 判断窗口句柄是否仍然有效。
     *
     * @param hwnd 窗口句柄
     * @return 有效返回 true
     */
    public boolean isWindowAlive(long hwnd) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT);
            out.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) isWindowAliveHandle.invokeExact(context, hwnd, out);
            if (rc != RC_OK) {
                return false;
            }
            return out.get(ValueLayout.JAVA_INT, 0L) != 0;
        } catch (Throwable t) {
            log.warn("uia_is_window_alive 调用异常: {}", t.getMessage());
            return false;
        }
    }

    /**
     * 按 JSON 选择器查找元素。
     *
     * @param selectorJson 选择器 JSON
     * @param maxCount     返回上限
     * @return 元素句柄数组（元素由原生侧元素池持有，用完需 {@link #release} 或 {@link #releaseAll()}）
     * @throws IllegalStateException 选择器非法时抛出
     */
    public long[] find(String selectorJson, int maxCount) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment sel = confined.allocateFrom(selectorJson, StandardCharsets.UTF_8);
            MemorySegment ids = confined.allocate(ValueLayout.JAVA_LONG, maxCount);
            MemorySegment countOut = confined.allocate(ValueLayout.JAVA_INT);
            countOut.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) findHandle.invokeExact(context, sel, ids, maxCount, countOut);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_find 失败, code=" + rc + ": " + lastError());
            }
            int total = countOut.get(ValueLayout.JAVA_INT, 0L);
            int n = Math.min(total, maxCount);
            long[] result = new long[n];
            for (int i = 0; i < n; i++) {
                result[i] = ids.getAtIndex(ValueLayout.JAVA_LONG, i);
            }
            if (total > n) {
                log.warn("uia_find 命中 {} 个元素，超过 maxCount={} 已截断", total, maxCount);
            }
            return result;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_find 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 查找单个元素。
     *
     * @param selectorJson 选择器 JSON
     * @return 元素句柄；未命中返回 0
     */
    public long findOne(String selectorJson) {
        long[] ids = find(selectorJson, 1);
        return ids.length == 0 ? 0L : ids[0];
    }

    /**
     * 批量读取元素属性快照。
     *
     * @param ids 元素句柄数组
     * @return JSON 数组字符串；入参非法时返回空数组
     */
    public String describe(long[] ids) {
        if (ids == null || ids.length == 0) {
            return "[]";
        }
        StringBuilder sb = new StringBuilder(ids.length * 4).append('[');
        for (int i = 0; i < ids.length; i++) {
            if (i > 0) {
                sb.append(',');
            }
            sb.append(ids[i]);
        }
        sb.append(']');
        return describeJson(sb.toString());
    }

    /**
     * 批量读取元素属性快照并反序列化为强类型对象。
     *
     * @param ids 元素句柄数组
     * @return 元素快照列表；顺序与入参一致
     */
    public java.util.List<UiaElementInfo> describeInfos(long[] ids) {
        if (ids == null || ids.length == 0) {
            return java.util.List.of();
        }
        String json = describe(ids);
        try {
            java.util.List<UiaElementInfo> list = UiaJson.MAPPER.readValue(json,
                    UiaJson.MAPPER.getTypeFactory()
                            .constructCollectionType(java.util.List.class, UiaElementInfo.class));
            return list == null ? java.util.List.of() : list;
        } catch (Exception e) {
            log.warn("uia_describe 结果反序列化失败: {}", e.getMessage());
            return java.util.List.of();
        }
    }

    /**
     * 以元素句柄 JSON 数组读取属性快照。
     *
     * @param idsJson 元素句柄 JSON 数组，如 {@code [1,2,3]}
     * @return JSON 数组字符串
     */
    public String describeJson(String idsJson) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment ids = confined.allocateFrom(idsJson, StandardCharsets.UTF_8);
            MemorySegment out = confined.allocate(ValueLayout.ADDRESS);
            out.set(ValueLayout.ADDRESS, 0L, MemorySegment.NULL);
            int rc = (int) describeHandle.invokeExact(context, ids, out);
            return readOutString(rc, out, "uia_describe");
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_describe 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 读取元素单个属性。
     *
     * @param id         元素句柄
     * @param propertyName 属性名，见 Rust 侧 {@code uia_get_property} 注释
     * @return 属性值
     */
    public String getProperty(long id, String propertyName) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment prop = confined.allocateFrom(propertyName, StandardCharsets.UTF_8);
            MemorySegment out = confined.allocate(ValueLayout.ADDRESS);
            out.set(ValueLayout.ADDRESS, 0L, MemorySegment.NULL);
            int rc = (int) getPropertyHandle.invokeExact(context, id, prop, out);
            return readOutString(rc, out, "uia_get_property");
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_get_property 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 读取元素屏幕矩形。
     *
     * @param id 元素句柄
     * @return 长度为 4 的数组 {@code [left, top, right, bottom]}，单位为物理像素
     */
    public int[] getRect(long id) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT, 4);
            for (int i = 0; i < 4; i++) {
                out.setAtIndex(ValueLayout.JAVA_INT, i, 0);
            }
            int rc = (int) getRectHandle.invokeExact(context, id, out);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_get_rect 失败, code=" + rc + ": " + lastError());
            }
            int[] rect = new int[4];
            for (int i = 0; i < 4; i++) {
                rect[i] = out.getAtIndex(ValueLayout.JAVA_INT, i);
            }
            return rect;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_get_rect 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 通过 {@code Value} 模式写入文本。
     *
     * <p>这是写文本的首选路径：不移动光标、不抢焦点、不产生逐键事件。</p>
     *
     * @param id   元素句柄
     * @param text 待写入文本
     * @return 写入成功返回 true
     */
    public boolean setValue(long id, String text) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment textSeg = confined.allocateFrom(text, StandardCharsets.UTF_8);
            return invokeElementWithText(setValueHandle, "uia_set_value", id, textSeg);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_set_value 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 通过 {@code Invoke} 模式触发控件。
     *
     * @param id 元素句柄
     * @return 触发成功返回 true
     */
    public boolean invoke(long id) {
        return invokeElement(invokeHandle, "uia_invoke", id);
    }

    /**
     * 将输入焦点设置到目标元素。
     *
     * @param id 元素句柄
     * @return 设置成功返回 true
     */
    public boolean setFocus(long id) {
        return invokeElement(setFocusHandle, "uia_set_focus", id);
    }

    /**
     * 激活目标窗口，把前台焦点交给它。
     *
     * <p>键盘输入类操作的前置条件：目标进程必须先成为前台进程，
     * 否则 Windows 的前台锁会直接丢弃按键事件。</p>
     *
     * @return 激活成功返回 true
     */
    public boolean activateWindow() {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT);
            out.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) activateWindowHandle.invokeExact(context, out);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_activate_window 失败, code=" + rc + ": " + lastError());
            }
            return out.get(ValueLayout.JAVA_INT, 0L) != 0;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_activate_window 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 以真实鼠标点击目标元素中心。
     *
     * <p>会移动光标，仅在元素既不支持 {@code Invoke} 又不支持 {@code Value} 时作为最后兜底。</p>
     *
     * @param id 元素句柄
     * @return 点击成功返回 true
     */
    public boolean click(long id) {
        return invokeElement(clickHandle, "uia_click", id);
    }

    /**
     * 按键串模拟。
     *
     * <p>语法：{@code {NAME}} 表示特殊键或修饰键（如 <code>{ENTER}</code>、<code>{CTRL}</code>），
     * 其余字符按 Unicode 下发。调用前需确保目标窗口已通过 {@link #activateWindow()} 成为前台。</p>
     *
     * @param keys 按键串，如 <code>"{CTRL}v"</code>
     * @return 下发成功返回 true
     */
    public boolean sendKeys(String keys) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment seg = confined.allocateFrom(keys, StandardCharsets.UTF_8);
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT);
            out.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) sendKeysHandle.invokeExact(seg, out);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_send_keys 失败, code=" + rc + ": " + lastError());
            }
            return out.get(ValueLayout.JAVA_INT, 0L) != 0;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_send_keys 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 写入 {@code CF_UNICODETEXT} 剪贴板。
     *
     * @param text 待写入文本
     * @return 写入成功返回 true
     */
    public boolean setClipboard(String text) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment seg = confined.allocateFrom(text, StandardCharsets.UTF_8);
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT);
            out.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) setClipboardHandle.invokeExact(seg, out);
            if (rc != RC_OK) {
                throw new IllegalStateException("uia_set_clipboard 失败, code=" + rc + ": " + lastError());
            }
            return out.get(ValueLayout.JAVA_INT, 0L) != 0;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_set_clipboard 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 导出控件树快照 JSON，用于诊断与选择器调优。
     *
     * @param maxDepth 最大深度，非正数取 12
     * @param maxNodes 最大节点数，非正数取 3000
     * @return JSON 字符串
     */
    public String dumpTree(int maxDepth, int maxNodes) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.ADDRESS);
            out.set(ValueLayout.ADDRESS, 0L, MemorySegment.NULL);
            int rc = (int) dumpTreeHandle.invokeExact(context, maxDepth, maxNodes, out);
            return readOutString(rc, out, "uia_dump_tree");
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException("uia_dump_tree 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 释放元素池中的单个元素句柄。
     *
     * @param id 元素句柄
     */
    public void release(long id) {
        try {
            int rc = (int) releaseHandle.invokeExact(context, id);
            if (rc != RC_OK) {
                log.warn("uia_release 返回非零码: {}", rc);
            }
        } catch (Throwable t) {
            log.warn("uia_release 调用异常: {}", t.getMessage());
        }
    }

    /**
     * 清空元素池。
     */
    public void releaseAll() {
        try {
            int rc = (int) releaseAllHandle.invokeExact(context);
            if (rc != RC_OK) {
                log.warn("uia_release_all 返回非零码: {}", rc);
            }
        } catch (Throwable t) {
            log.warn("uia_release_all 调用异常: {}", t.getMessage());
        }
    }

    /**
     * 获取最近一次原生错误信息（线程局部，无需释放）。
     *
     * @return 错误信息；无错误时返回 null
     */
    public String lastError() {
        try {
            MemorySegment pointer = (MemorySegment) lastErrorHandle.invokeExact();
            if (pointer == null || pointer.address() == 0L) {
                return null;
            }
            return pointer.reinterpret(Long.MAX_VALUE).getString(0L, StandardCharsets.UTF_8);
        } catch (Throwable t) {
            log.warn("uia_last_error 调用异常: {}", t.getMessage());
            return null;
        }
    }

    /**
     * 关闭桥接器：先销毁 COM 上下文，再释放符号表会话。
     */
    @Override
    public void close() {
        if (created) {
            try {
                int rc = (int) destroyHandle.invokeExact(context);
                if (rc != RC_OK) {
                    log.warn("uia_destroy 返回非零码: {}", rc);
                }
            } catch (Throwable t) {
                log.warn("uia_destroy 调用异常: {}", t.getMessage());
            }
        }
        arena.close();
    }

    /**
     * 调用签名为 {@code (int64 ctx, int64 id, void* out)} 的原生函数。
     *
     * @param handle 函数句柄
     * @param name   函数名（异常信息用）
     * @param id     元素句柄
     * @return 出参非 0 返回 true
     */
    private boolean invokeElement(MethodHandle handle, String name, long id) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT);
            out.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) handle.invokeExact(context, id, out);
            if (rc != RC_OK) {
                throw new IllegalStateException(name + " 失败, code=" + rc + ": " + lastError());
            }
            return out.get(ValueLayout.JAVA_INT, 0L) != 0;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException(name + " 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 调用签名为 {@code (int64 ctx, int64 id, const char* text, void* out)} 的原生函数。
     *
     * @param handle  函数句柄
     * @param name    函数名（异常信息用）
     * @param id      元素句柄
     * @param textSeg 已分配在 confined arena 中的字符串入参
     * @return 出参非 0 返回 true
     */
    private boolean invokeElementWithText(MethodHandle handle, String name, long id,
                                          MemorySegment textSeg) {
        try (Arena confined = Arena.ofConfined()) {
            MemorySegment out = confined.allocate(ValueLayout.JAVA_INT);
            out.set(ValueLayout.JAVA_INT, 0L, 0);
            int rc = (int) handle.invokeExact(context, id, textSeg, out);
            if (rc != RC_OK) {
                throw new IllegalStateException(name + " 失败, code=" + rc + ": " + lastError());
            }
            return out.get(ValueLayout.JAVA_INT, 0L) != 0;
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException(name + " 调用异常: " + t.getMessage(), t);
        }
    }

    /**
     * 调用无返回值且只接收上下文句柄的原生函数。
     *
     * @param handle 函数句柄
     * @param name   函数名（异常信息用）
     * @param ctx    上下文句柄
     */
    private void invokeVoid(MethodHandle handle, String name, long ctx) {
        try {
            int rc = (int) handle.invokeExact(ctx);
            if (rc != RC_OK) {
                log.warn("{} 返回非零码: {}", name, rc);
            }
        } catch (Throwable t) {
            log.warn("{} 调用异常: {}", name, t.getMessage());
        }
    }

    /**
     * 读取原生出参指针指向的字符串并释放原生内存。
     *
     * @param rc           原生函数返回码
     * @param outSlot      出参指针槽
     * @param functionName 函数名（异常信息用）
     * @return UTF-8 字符串；原生返回空指针时返回 null
     */
    private String readOutString(int rc, MemorySegment outSlot, String functionName) {
        if (rc != RC_OK) {
            throw new IllegalStateException(functionName + " 失败, code=" + rc + ": " + lastError());
        }
        MemorySegment pointer = outSlot.get(ValueLayout.ADDRESS, 0L);
        if (pointer == null || pointer.address() == 0L) {
            return null;
        }
        // 原生字符串以 '\0' 结尾，放开段边界后按 UTF-8 读取
        String result = pointer.reinterpret(Long.MAX_VALUE).getString(0L, StandardCharsets.UTF_8);
        freeString(pointer);
        return result;
    }

    /**
     * 释放原生层分配的字符串。
     *
     * @param pointer 原生字符串指针
     */
    private void freeString(MemorySegment pointer) {
        try {
            freeStringHandle.invokeExact(pointer);
        } catch (Throwable t) {
            log.warn("uia_free_string 调用异常: {}", t.getMessage());
        }
    }

    /**
     * 绑定有返回值的原生函数。
     *
     * @param lookup       符号查找表
     * @param name         函数符号名
     * @param returnLayout 返回值布局
     * @param argLayouts   参数布局
     * @return 下行调用句柄
     */
    private static MethodHandle bind(SymbolLookup lookup, String name,
                                     ValueLayout returnLayout, ValueLayout... argLayouts) {
        MemorySegment symbol = lookup.find(name)
                .orElseThrow(() -> new IllegalStateException(LIBRARY_NAME + " 动态库缺少导出符号: " + name));
        return LINKER.downcallHandle(symbol, FunctionDescriptor.of(returnLayout, argLayouts));
    }

    /**
     * 绑定无返回值的原生函数。
     *
     * @param lookup     符号查找表
     * @param name       函数符号名
     * @param argLayouts 参数布局
     * @return 下行调用句柄
     */
    private static MethodHandle bindVoid(SymbolLookup lookup, String name, ValueLayout... argLayouts) {
        MemorySegment symbol = lookup.find(name)
                .orElseThrow(() -> new IllegalStateException(LIBRARY_NAME + " 动态库缺少导出符号: " + name));
        return LINKER.downcallHandle(symbol, FunctionDescriptor.ofVoid(argLayouts));
    }
}
