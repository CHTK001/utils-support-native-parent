# utils-support-native-uia

Windows UI Automation 通用原生库（自研 Rust + UIA COM 绑定，JSON 选择器引擎）。

## 概述

本模块提供 **通用 UIA 原语**：窗口定位、控件树遍历、属性读取、UIA 模式操作（`Value` / `Invoke`）、
键盘与剪贴板输入。

**设计约束：只做外部进程级的 UIA 客户端** —— 不注入、不改写目标进程内存、不实现私有协议。
所有能力通过系统暴露的 `IUIAutomation` COM 接口获取，在风控视角下等价于"一个人在看屏幕并操作鼠标键盘"。

**不含任何 IM 业务语义。** 会话、消息、收信人等概念由上层模块通过 `UiaSelector`（JSON 选择器）
描述控件结构后自行组装。本模块可被 IM 自动化、桌面运维、RPA 等任意场景复用。

## 为什么是 Rust 而不是 Python

- 本仓库大量模块已采用 **Rust 动态库 + Java 25 FFM** 通路（`native-wechat` / `native-headless` /
  `native-ffmpeg` / `native-nmap` 等），风格与构建链路完全一致。
- `IUIAutomation` 等 COM 接口只在 `windows` crate（非 `windows-sys`）中生成。
- GraalVM native-image **不支持嵌入 CPython**，走 Python 会与仓库的 native-image 配置冲突；
  且 JVM 无法通过 FFM 直接访问 Python 对象，跨边界只能传字符串，相比直接写 C ABI 多一层无谓开销。

## 模块结构

```
utils-support-native-uia/
├── pom.xml
├── src/
│   ├── main/java/com/chua/nativeuia/
│   │   └── support/
│   │       ├── UiaBridge.java          # Java FFM 桥接器
│   │       ├── UiaSelector.java        # JSON 选择器 POJO
│   │       ├── UiaElementInfo.java     # 元素属性快照
│   │       └── UiaJson.java            # JSON 门面
│   ├── main/rust/                      # Rust FFI 库
│   │   ├── Cargo.toml
│   │   ├── build.sh
│   │   └── src/
│   │       ├── lib.rs                  # 导出 C ABI + 遍历/匹配核心
│   │       ├── selector.rs             # 选择器模型与控件类型表
│   │       ├── input.rs                # SendKeys / 剪贴板 / 鼠标点击
│   │       ├── error.rs                # 线程局部错误
│   │       └── bin/selftest.rs         # Rust 自测
│   └── main/resources/
│       ├── native/windows-x86_64/uia_rust.dll
│       └── META-INF/native-image/com.chua/utils-support-native-uia/
```

## 构建

```bash
# 编译 Rust 动态库（默认离线，优先使用本地 registry 缓存）
cd src/main/rust
./build.sh                      # 自动检测平台
./build.sh windows x86_64 release
# 产物自动复制到 src/main/resources/native/windows-x86_64/

# Maven 编译安装
mvn install -DskipTests
```

需要 Java 25 编译插件与 `-enable-native-access` / `--enable-preview` 参数（已在 `pom.xml` 中配置）。
运行期同样需要：

```bash
java --enable-native-access=ALL-UNNAMED --enable-preview ...
```

## 平台支持

**UIA 是 Windows 独有的 COM API**（`IUIAutomation` 定义在 `UIAutomationCore.dll`），
因此本模块的平台矩阵**有意保持单平台**，不产出 Linux / macOS 产物。

`src/main/rust/src/*.rs` 直接使用 `windows::Win32::UI::Accessibility`，**未做 `#[cfg(windows)]` 门控**，
在非 Windows 目标上无法编译 —— 这是上游 API 决定的硬约束，不是构建配置缺失。
强行产出空壳 `.so` / `.dylib` 只会得到零导出符号的产物，Java 侧 FFM 绑定必然失败。

Java 侧 `UiaBridge.isSupported()` 在非 Windows 平台直接返回 `false` 并给出明确错误。

当前预编译动态库仅 `windows-x86_64`，随 jar 打包在 `/native/windows-x86_64/` 下，
`UiaBridge.load()` 时自动从 classpath 抽取并加载。

> 若需 Linux / macOS 的界面自动化，应使用 AT-SPI / AX API，那是独立实现，与本模块不共享代码。

## 使用

### 基础流程

```java
try (UiaBridge bridge = UiaBridge.load()) {
    // 1) 绑定目标窗口：同进程多个同名窗口时，原生层取面积最大的
    long hwnd = bridge.attachWindow("微信", "Qt", true);

    // 2) 用 JSON 选择器查找元素
    long edit = bridge.findOne(UiaSelector.of("Edit")
            .setRequireEnabled(true)
            .setMaxDepth(8)
            .toJson());

    // 3) 写文本（首选 Value 模式：不抢焦点、不产生逐键事件）
    bridge.setValue(edit, "你好");

    // 4) 读回属性确认
    System.out.println(bridge.getProperty(edit, "name"));
    int[] rect = bridge.getRect(edit);
    bridge.release(edit);
}
```

### 选择器

所有字符串字段留空表示"不约束该维度"。

| 字段 | 说明 |
|------|------|
| `controlType` | 控件类型，如 `ListItem` / `Edit` / `Text` / `Button` |
| `name` / `nameRegex` | 名称精确 / 正则匹配（Rust `regex` 语法，无环视与反向引用） |
| `automationId` / `className` | AutomationId / 窗口类名精确匹配 |
| `processId` | 宿主进程 ID |
| `requireEnabled` / `requireOnscreen` | 是否要求可用 / 在屏 |
| `maxDepth` | 相对根元素的遍历深度上限 |
| `children` | 必须存在的后代选择器（全部满足才命中） |
| `ancestor` / `childDepth` | 祖先选择器与向上/向下跨越层数 |
| `index` | 命中后取第几个；`null` 全给，`-1` 取末个 |

```java
UiaSelector.of("ListItem")
        .setNameRegex("^\\(3\\)张三$")
        .withChild(UiaSelector.of("Text"))
        .setChildDepth(4)
        .setRequireOnscreen(true)
        .toJson();
```

### 调优：导出控件树

控件层级不准时，先导出真实树再改选择器 —— **不要凭猜**：

```java
String tree = bridge.dumpTree(20, 4000);
Files.write(Path.of("uia_tree.json"), tree.getBytes(UTF_8));
```

或直接用 Rust 自测：

```bash
./target/x86_64-pc-windows-msvc/release/uia_selftest.exe "微信" 20 4000
```

## C ABI 接口

| 函数 | 说明 |
|------|------|
| `uia_create` / `uia_destroy` | 创建 / 销毁 UIA 上下文（内部 `CoInitializeEx(STA)`） |
| `uia_attach_window` | 按标题子串 + 可选类名绑定窗口，取面积最大的命中项 |
| `uia_detach_window` | 解绑，回到桌面根元素 |
| `uia_is_window_alive` | 判断窗口句柄是否仍有效 |
| `uia_find` | 按 JSON 选择器查找元素，结果登记进元素池 |
| `uia_describe` | 批量读取元素属性快照 JSON |
| `uia_get_property` / `uia_get_rect` | 读取元素单个属性 / 屏幕矩形 |
| `uia_set_value` | `Value` 模式写文本（首选路径） |
| `uia_invoke` / `uia_set_focus` | `Invoke` 模式触发 / 设置焦点 |
| `uia_activate_window` | 把前台焦点交给目标窗口（键盘输入的前置条件） |
| `uia_click` | 真实鼠标点击（会移动光标，仅作最后兜底） |
| `uia_release` / `uia_release_all` | 释放元素池句柄 |
| `uia_dump_tree` | 导出控件树快照 JSON |
| `uia_send_keys` | 按键串模拟，语法 `{ENTER}` / `{CTRL}v` |
| `uia_set_clipboard` | 写 `CF_UNICODETEXT` 剪贴板（中文输入推荐路径） |
| `uia_free_string` | 释放本库返回的字符串 |
| `uia_last_error` | 最近一次错误信息（线程局部） |

## 关键约束

**线程模型**：`uia_create` 内部 `CoInitializeEx(STA)`，COM 单元与线程绑定，
因此 `UiaBridge` 实例**必须与创建它的线程同生命周期，不可跨线程使用**。需要多线程时每线程各建一个。

**窗口可见性**：目标窗口**最小化时，内部控件不会被实例化**，
UIA 只能看到 3~4 个节点（`Window` + 标题栏 + 渲染子窗），读不到任何业务控件。
这是 Qt / Electron 类客户端的共同行为，不是缺陷。运行前必须确认窗口可见。

**写文本的优先级**：`Value` 模式（不抢焦点，最优）→ 剪贴板 + `Ctrl+V`（中文输入推荐）→ `SendInput` 逐字模拟（最差）。
**不要用 `PostMessage` 直接改目标控件** —— 那已属于注入行为。

**元素池**：`uia_find` 返回的 `id` 是原生元素池下标 + 1，调用方负责 `uia_release` / `uia_release_all` 回收。

## 上层用例

- `utils-support-native-wechat` 的 `com.chua.nativewechat.uia.WechatUiaPollDirectory`
  —— 微信 3.9.x 会话轮询与回信，构建在本模块之上。
