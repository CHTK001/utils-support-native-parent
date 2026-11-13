# utils-support-native-rdp

RDP 远控桥接库。Java 侧不接入 Guacamole，通过 JNA 调用平台自研 C 扩展 `rdpj`（基于
FreeRDP 3.x），在主机侧连出到 RDP 服务器，远端桌面按帧推送给浏览器 Canvas，键鼠事件回传
文本帧。

本模块同时是 **RDP 原生库的唯一归属地**：`spring-support-server-starter` 通过 Maven 依赖
本模块获得 `RdpjBridge` 与随包的 `rdpj.dll`，不再自带 `native/rdpj` 资源与桥接类副本。

---

## 目录结构

```
utils-support-native-rdp/
├── build.sh                          # Linux/macOS 构建（gcc/clang，自动探测 pkg-config）
├── build.bat                         # Windows 构建（MSVC cl.exe，依赖 vcpkg）
├── pom.xml                           # jna / jna-platform + utils-support-native-loader
└── src/main
    ├── c/rdpj/
    │   ├── rdpj-win32.c              # 打出现行 rdpj.dll 的源码（Windows + FreeRDP 静态链接）
    │   ├── README-win32.md           # Windows 构建步骤与导出符号清单
    │   ├── rdpj.c                    # 跨平台设想版本（gcc，含 rdpj_status/rdpj_errno，未产出）
    │   └── rdpj.h                    # 与 rdpj.c 配套的头文件（注意：不是 DLL 的 ABI）
    ├── java/com/chua/nativerdp/support/
    │   └── RdpjBridge.java           # JNA 绑定 + classpath 解包/加载（与 DLL 的 ABI 一致）
    └── resources/native/rdpj/win32-x86-64/
        ├── rdpj.dll                  # 12,873,802 字节，静态链接 FreeRDP 3.32 / WinPR / OpenSSL / zlib
        └── rdpj.index                # 资源清单（逐行列出需要解包的 DLL）
```

## 加载方式

`RdpjBridge` 不直接 `System.loadLibrary`，按以下顺序解析：

1. 系统属性 `chua.rdpj.dir` 或环境变量 `CHUA_RDPJ_DIR` 指定的目录（目录内需有 `rdpj.dll`）；
2. classpath 资源 `native/rdpj/win32-x86-64/`，按 `rdpj.index` 解包到
   `${java.io.tmpdir}/chua-rdpj/win32-x86-64/`，再 `Native.load(绝对路径, Rdpj.class)`。

加载前会调用 `kernel32!SetDllDirectoryW` 把解包目录加入 DLL 搜索路径，方便替换版原生库
带上自己的依赖。加载失败不阻断 JVM 启动：`RdpjBridge.available()` 返回 `false`，
`instance()` 抛出带原因描述的 `IllegalStateException`，调用方据此拒绝 RDP 连接。

## 构建

需要 **FreeRDP 3.x 开发包**（`freerdp/freerdp.h` + `libfreerdp3.so/.lib/.dylib` + `winpr`）。

### Windows

产物 `rdpj.dll` 的完整构建步骤（MSYS2 mingw64 + FreeRDP 3.32.1 静态编译）见
`src/main/c/rdpj/README-win32.md`。要点是 `-D__STDC_NO_THREADS__=1`、`-DFREERDP_EXPORTS`、
静态链接 FreeRDP/WinPR/OpenSSL/zlib，最终只依赖 Windows 系统 DLL。

### Linux / macOS

`build.sh` 使用 `rdpj-win32.c`（WinPR 兼容层让同一份源码在 POSIX 上编译），
通过 pkg-config 探测 FreeRDP 3.x，输出
`src/main/resources/native/linux-x86_64/librdpj.so` /
`darwin-*/librdpj.dylib`。CI：`.github/workflows/native-rdp.yml`。

Linux 依赖 `freerdp3-dev`，macOS 依赖 `brew install freerdp`；产物默认动态链接
FreeRDP，部署机需具备对应的 `libfreerdp3` / `libfreerdp-client3` / `libwinpr3`。

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

- 已产出：Windows x86_64 的 `rdpj.dll`（静态链接 FreeRDP 3.32.1），随本模块 jar 分发；
  Linux x86_64 / macOS aarch64 由 GitHub Actions 构建 `librdpj.so` / `librdpj.dylib`。
- 已实现：C 源码（`rdpj-win32.c`，Windows/POSIX 共用）、JNA 桥接（`RdpjBridge`）、
  后端 WebSocket 处理器（`RdpDirectWebSocketHandler` 逐帧推送 + 键鼠回传 + 连接清理）
  + 直连页 `/ms/server/remote/rdp-direct.html`。
- macOS x86_64 暂无托管 Intel runner，CI 只做源码构建检查，未产出 dylib。

## 占位错误约定

前端连接时若 `rdpj_create()` 调失败（库未加载），后端通过 WS 文本帧返回 `{"c":"error","msg":"..."}`。
前端 `ScRdpDirectViewer` 将错误展示在画布上方的状态栏中。

## 依赖关系

```
utils-support-native-rdp 4.0.0.43
├── jna 5.19.1 + jna-platform 5.19.1（使用方若声明更低版本，以使用方为准）
└── utils-support-native-loader 4.0.0.43（native 家族通用的抽取/加载工具）
```

使用方：`spring-support-parent-starter` 的 `spring-support-server-starter`，
依赖版本由该模块 `pom.xml` 的 `native.rdp.version` 指定（独立于 `utils.version`）。
