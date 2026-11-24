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
```

## 重建原生库（Windows）

```powershell
pwsh -NoProfile -File src/main/native/build.ps1
```

依赖本机工具链：dotnet SDK 10.0.100（`D:\apps\dotnet`，便携解压）+ MSVC 14.44
（`D:\VSBuildTools2`，提供链接器）。PaperSharp 及其依赖从 nuget.org 还原。

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
