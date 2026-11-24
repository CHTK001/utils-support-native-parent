# utils-support-native-papersharp

HTML(+CSS) → **分页 PDF / PNG / JPEG / WebP / SVG / ICO** 的文件转换模块，基于
[PaperSharp](https://github.com/SpringHgui/papersharp)（MIT）引擎的 **.NET 10 Native AOT 动态库**。

无浏览器、无 Office、无外部进程：渲染在 JVM 进程内经 FFM（Panama）调用完成。

## 能力

| 目标格式 | 说明 |
|---|---|
| `pdf` | 分页 PDF（@page 页边距/尺寸、页眉页脚、跨页续表 + thead 重放、break-inside/orphans/widows、中文字体子集嵌入） |
| `png` / `jpeg` / `webp` | 整页长图（布局视口默认 794 CSS px ≈ A4@96dpi） |
| `svg` | 矢量整页图 |
| `ico` | 图标（默认 32px 条目） |

源格式：`html` / `htm` / `xhtml`。中英文混排、CJK 逐字折行为引擎强项。

## 使用

```java
// 统一文件转换入口（SPI 自动发现本模块实现 @Spi("papersharp")）
ConvertSupport.convert("html", "pdf")
        .from("report.html")
        .to("report.pdf")
        .convert();
```

CSS 约定：HTML 内 `<style>` / `style` 属性照常生效；打印规则（@page 等）写在
HTML 的 style 标签内。单文件契约，不读取外部 .css 伴生文件。

## 结构

```
src/main/java/com/chua/papersharp/support/
  PaperSharpNativeBridge.java    FFM 桥：加载 AOT 库、绑定 C ABI、编组调用
  PapersharpFileConvertSystem.java  FileConvertSystem SPI 实现（@Spi("papersharp")）
src/main/native/                 AOT 工程源码与构建脚本（可重编）
  NativeHost/NativeHost.csproj   net10.0，PublishAot，引用 NuGet PaperSharp 1.0.5
  NativeHost/Exports.cs          C ABI 导出面（papersharp_render_* / papersharp_free）
  build.ps1                      Windows 构建脚本：dotnet publish + 拷贝产物到 resources
src/main/resources/
  META-INF/extensions/com.chua.common.support.file.converter.FileConvertSystem  SPI 注册
  native/windows-x86_64/         NativeHost.dll + libSkiaSharp.dll + libHarfBuzzSharp.dll
  native/linux-x86_64/           NativeHost.so + libSkiaSharp.so + libHarfBuzzSharp.so
  native/darwin-x86_64/          NativeHost.dylib + libSkiaSharp.dylib + libHarfBuzzSharp.dylib
  native/darwin-aarch64/         NativeHost.dylib + libSkiaSharp.dylib + libHarfBuzzSharp.dylib
```

## 重建原生库（Windows）

```powershell
pwsh -NoProfile -File src/main/native/build.ps1
```

依赖本机工具链：dotnet SDK 10.0.100（`D:\apps\dotnet`，便携解压）+ MSVC 14.44
（`D:\VSBuildTools2`，提供链接器）。PaperSharp 及其依赖从 nuget.org 还原。

## 重建原生库（四平台 CI，正式口径）

四平台（windows-x86_64 / linux-x86_64 / darwin-x86_64 / darwin-aarch64）产物由
GitHub Actions 工作流 `.github/workflows/native-papersharp.yml` 构建（push 到
main 或手动 workflow_dispatch 触发），四条腿全绿后自动把产物回填到上述
`native/<platform>/` 目录。linux 主库已补 `$ORIGIN` RUNPATH，三个库同目录
即可完成依赖解析，无需额外加载路径。

## 运行时依赖（按平台）

- **Windows x86_64**：无外部运行时依赖。
- **Linux x86_64**：glibc 环境即可（已在 `eclipse-temurin:25-jre` 容器全链路
  验收：Probe 六格式 + Smoke 全 PASS）。**无需 libicu** —— AOT 构建期已启用
  `InvariantGlobalization`；缺 ICU 的宿主上 .NET 会 FailFast 直接 abort 宿主
  JVM 进程，且运行时环境变量无法补救（AOT 配置内嵌），必须构建期切换。
  Skia 字体解析走 fontconfig；验收容器装有 fontconfig 与 CJK 字体面，
  无 CJK 字体的极简环境中文表现未实测。
- **macOS x86_64 / arm64**：无外部运行时依赖；`libSkiaSharp.dylib` /
  `libHarfBuzzSharp.dylib` 为双架构通用二进制，`NativeHost.dylib` 按架构单出。

## 运行时加载

`PaperSharpNativeBridge` 经 `NativeLoader` 从 classpath `native/<platform>/` 提取
三个动态库到临时目录并按文件名排序加载（`libHarfBuzzSharp` → `libSkiaSharp` →
`NativeHost`，依赖库先入进程模块表），再以 `SymbolLookup.libraryLookup` 绑定符号。
加载失败时 `isLoaded()` 为 false，SPI 转换快速失败并携带原因，不影响宿主启动。

内存约定：输出字节块与错误串由 native 侧分配，Java 侧读取后经 `papersharp_free`
归还（输出尺寸事先未知，无法由 Java 预分配）。

## 许可证

- 本模块 Java 代码：随工作区主许可
- PaperSharp：MIT（上游仓库 github.com/SpringHgui/papersharp）
- 内嵌字体 Noto Sans SC 子集：OFL
- SkiaSharp / HarfBuzz 原生库：MIT
