# utils-support-native-rdp

RDP 远控桥接库。Java 侧不接入 Guacamole，通过 JNA 调用平台自研 C 扩展 `librdpj`（基于
FreeRDP 3.x），在主机侧连出到 RDP 服务器，远端桌面按帧推送给浏览器 Canvas，键鼠事件回传
文本帧。

---

## 目录结构

```
utils-support-native-rdp/
├── build.sh                     # Linux/macOS 构建（gcc/clang，自动探测 pkg-config）
├── build.bat                    # Windows 构建（MSVC cl.exe，依赖 vcpkg）
├── pom.xml                      # 依赖 jna/jna-platform + utils-support-common-starter
└── src/main
    ├── c/rdpj/rdpj.c            # rdpj API 实现（连接/帧/鼠标/键盘/状态）
    ├── c/rdpj/rdpj.h            # 导出符号声明
    ├── java/com/chua/nativerdp/support/
    │   ├── RdpjBridge.java      # JNA 绑定 + NativeLoader 抽取/加载
    │   └── ...
    └── resources/native/
        ├── darwin-aarch64/      # librdpj.dylib 放入处
        ├── darwin-x86_64/
        ├── linux-x86_64/        # librdpj.so 放入处
        └── windows-x86_64/      # rdpj.dll 放入处
```

_native_ 目录下：Java 侧不直接 `System.loadLibrary`，而是 `NativeLoader.of("chua_native_rdpj")`
从 `native/<os-arch>/` 中提取匹配 `*rdpj*` 的文件到临时目录并
`Native.load(abs, Rdpj.class)`。因此产物必须放在
**src/main/resources/native/<os>-<arch>/**，且文件名为：
*   Linux: `librdpj.so`
*   macOS  (darwin): `librdpj.dylib`
*   Windows: `rdpj.dll`

## 构建（四平台）

需要 **FreeRDP 3.x 开发包**（`freerdp/freerdp.h` + `libfreerdp3.so/.lib/.dylib` + `winpr`）。

### Linux → build.sh

```bash
chmod +x build.sh
./build.sh
# 或使用已有 SDK：
./build.sh --sdk /usr/local/freerdp
```

默认自动探测 `pkg-config freerdp3`（`/usr/local/lib/freerdp3` / `/opt/freerdp`）；未找到
则从 GitHub 拉取 FreeRDP 3.3.0 源码、cmake 到 `/tmp/freerdp-sdk`（约 3-8 分钟网络 + 20 分钟
编译），输出 `src/main/resources/native/linux-x86_64/librdpj.so`。

### macOS → build.sh

```bash
./build.sh --sdk $(brew list freerdp --prefix)   # Homebrew 安装的 FreeRDP
```

Homebrew 未提供开发头时，自行编译 FreeRDP 3.x 到 ~/.freerdp-sdk（参照 pgk），输出
`src/main/resources/native/darwin-x86_64/librdpj.dylib` / `darwin-aarch64/librdpj.dylib`。

### Windows → build.bat

```cmd
build.bat --sdk C:\vcpkg\installed\x64-windows-static
```

vcpkg 先 `vcpkg install freerdp:x64-windows-static`，产物为 `rdpj.dll`（MSVC `cl.exe /LD`）。
没有 `cl.exe` 时本机电不能执行，请使用 WSL2 / Mac 编译后交出产物。

### 清理

```bash
./build.sh --clean
```

## 帧约定（Java ↔ C）

Java 侧通过 `/ws/ms/server/terminal/rdp` 创建 WebSocket 连接后交互。

| 项 | 格式 |
|---|---|
| 视频帧 | 32 字节头（`uint32_t x8`：width/height/stride/bpp/x/y/cx/cy）+ 每行 stride 字节的 BGRA 扫描线（自底向上） |
| 鼠标 | 文本帧 `{"type":"mouse","x":..,"y":..,"button":..,"down":..}` |
| 键盘 | 文本帧 `{"type":"keyboard","keysym":..,"down":..}` （keysym 为 RDP 扫描码） |
| 屏幕 | 文本帧 `{"type":"resize","width":..,"height":..}` |

## 当前状态

已实现：C 源码（`rdpj.c`）、JNA 桥接（`RdpjBridge`/`Rdpj` 接口）、后端 WebSocket 处理器
（`RdpDirectWebSocketHandler` 逐帧推送 + 键鼠回传 + 连接清理）+ 直连页
`/ms/server/remote/rdp-direct.html`。
未完成：`native/` 目录下无已编译产物（本机无 C 工具链），`RdpjBridge.isLoaded()` 为 `false`
时后端拒绝 RDP 连接并下发电错误帧。

## 占位错误约定

前端连接时若 `rdpj_create()` 调失败（库未加载），后端通过 WS 文本帧返回 `{"c":"error","msg":"..."}`。
前端 `ScRdpDirectViewer` 将错误展示在画布上方的状态栏中。

## 依赖关系

```
utils-support-native-rdp
└── utils-support-common-starter 4.0.0.50 (NativeLoader/NativeUtils)
    └── jna 5.19.1 + jna-platform 5.19.1
```
