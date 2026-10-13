# utils-support-native-imageopt

图片压缩 native 库——把 **libdeflate**（deflate/zlib 压缩内核）与
**libimagequant**（pngquant 的量化内核）收敛成扁平 C ABI，供 Java 侧通过 Panama FFM 直接下探调用。

动态库名：`chua_native_imageopt`（`chua_native_imageopt.dll` / `libchua_native_imageopt.so` / `libchua_native_imageopt.dylib`）

## 为什么是这个组合

图片压缩的收益有两个来源，本模块各接管一个：

| 来源 | 纯 Java 能力上限 | 本模块的方案 |
|---|---|---|
| **deflate 压缩 IDAT** | `java.util.zip.Deflater` 绑的是 zlib 本体，压缩率已到 zlib 上限 | libdeflate 的 **11~12 档**用 near-optimal 解析，实测比 zlib 最优档再小 **7%~13%**（PNG-8 索引数据 −7%~−8%，真彩色 −9%~−13%） |
| **调色板量化** | Lab 中位切分 + Floyd-Steinberg，大图 0.5~2 秒，且大色块会被量化出色带 | libimagequant 的 Oklab 迭代细化 + 有序抖动，同质量下快一个数量级，误差低两个数量级 |

JPEG 路径不在本模块范围：`utils-support-image-starter` 已有 `utils-support-native-libjpeg-turbo` 提供 TurboJPEG。

## 不需要 CMake

两个上游 C 库的源码随 crates.io 上的 crate 一起分发：

| Rust 依赖 | 内嵌的 C 库 | 版本 |
|---|---|---|
| `libdeflater` | libdeflate | 1.26.1 |
| `imagequant` | libimagequant（pngquant 内核） | 4.4.1 |

两者都由 `cc` crate 在构建机本地编译并**静态链接**进本 cdylib。目标机无需安装任何 C 库，
构建机也无需 CMake / nasm / 厂商 tarball——这与 `utils-support-native-libjpeg-turbo`
（需要 CMake + nasm + 源码 tarball）形成对比。

## 导出符号

| 符号 | 说明 |
|---|---|
| `chua_df_zlib_compress(src, len, level, dst, cap, len*)` | libdeflate 单次 zlib 压缩，level 0~12 |
| `chua_df_zlib_compress_best(src, len, level, attempts, dst, cap, len*)` | 在目标级别上下各取一档试压，返回最短结果 |
| `chua_iq_quantize(rgba, n, w, h, colors, dither, speed, pal*, palCap, idx*, count*)` | libimagequant 量化 |
| `chua_imageopt_version` | 门面版本串 |
| `chua_imageopt_deflate_version` | 内嵌 libdeflate 版本 |
| `chua_imageopt_quantize_version` | 内嵌 libimagequant 版本 |
| `chua_imageopt_last_error` | 最近一次失败原因（线程局部） |

### 约定

- 所有函数成功返回 `0`，失败返回负错误码；原因用 `chua_imageopt_last_error()` 取回。
- **不分配任何需要调用方释放的缓冲区**。压缩输出与量化输出都由调用方预分配、按容量上限传入。
  这样跨 FFM 边界不存在所有权歧义，也避免「谁负责 free」在 Java 侧被漏掉导致的原生内存泄漏。
- 容量不足返回 `ERR_TOO_SMALL(-2)`，并把实际所需字节数写进 `*len`，调用方据此扩容重试。
- 错误码：`-1` 入参非法、`-2` 缓冲区不足、`-3` 压缩失败、`-4` 量化失败、`-5` 内部 panic（已捕获）。

### 两处值得注意的实现选择

1. **目标已是最高档时不再向下试压。** libdeflate 的压缩率对级别单调，更低的档必然更大。
   `chua_df_zlib_compress_best(x, n, 12, 3)` 实际只试 1 档——否则每次 L3~L6 的压缩
   都要白白多压一遍还拿不到更小的结果。
2. **`chua_iq_quantize` 捕获 panic。** libimagequant 内部出错可能 panic，
   跨 FFM 边界的 unwind 会直接终止 JVM，因此用 `catch_unwind` 包住并返回 `-5`。

## 构建

```bash
# 宿主平台（CI 用这个）
cd src/main/rust
./build.sh                     # Linux / macOS
powershell -File build.ps1     # Windows
# 或跨架构
./build.sh linux aarch64
```

产物会落到 `src/main/resources/native/<platform>/`。

> **Windows 交叉编译**：默认构建宿主目标、用 rustup 默认工具链。
> 若本机默认工具链无法链接（例如没装 Visual Studio Build Tools、缺 `link.exe`），
> 显式指定 MinGW 宿主：
> ```powershell
> powershell -File build.ps1 -RustTarget x86_64-pc-windows-gnu -Toolchain stable-x86_64-pc-windows-gnu
> ```

### Cargo.lock 必须入库

根 `.gitignore` 有 `**/Cargo.lock`（本仓库交付的是**编好的动态库**而非库源码，
锁文件不入库意味着每次 CI 都在重新解依赖、产物不可复现）。本模块已加例外：

```gitignore
!utils-support-native-imageopt/src/main/rust/Cargo.lock
```

## CI

`.github/workflows/native-imageopt.yml`，四个平台各自**原生构建**（不交叉）：
`windows-x86_64` / `linux-x86_64` / `darwin-aarch64` 跑完整 FFM 冒烟；
`darwin-x86_64` 因托管 runner 已无 Intel 镜像，只验 Mach-O 架构与导出符号。

冒烟用一段内联的 Java（不依赖任何项目类，避免把 `utils-support-common-starter`
和 Lombok 拖进这个刻意保持零依赖的 workflow）直接绑这 7 个符号，断言：

- 版本串与内嵌的两个上游版本号可读；
- deflate 输出首字节为 `0x78`（zlib CMF），且能被 JDK `Inflater` **字节级还原**；
- 缓冲区不足时返回 `ERR_TOO_SMALL` 并回填所需字节数；
- 多档试压不劣于单档；最高档试压与单档结果一致（验证上面第 1 条选择）；
- 量化产出的索引全部落在调色板范围内（越界会让 Java 侧构造 `IndexColorModel` 时炸掉）；
- `max_colors` / `speed` / `pixel_count` 非法时被拒而非静默产出垃圾。

产物按需回提交到分支 `build/imageopt-native-artifacts`，由维护者合并。

## Java 侧接入

FFM 绑定类在本模块：`com.chua.nativeimageopt.support.ImageOptBridge`。

```java
if (!ImageOptBridge.isLoaded()) {
    // 动态库缺失，业务侧应降级，不要抛异常
    return ImageOptBridge.getLoadError();
}
byte[] zlib = ImageOptBridge.zlibCompress(bytes, 12);
ImageOptBridge.Quantized q = ImageOptBridge.quantize(rgba, w, h, 256, 100, 4);
```

SPI 适配类在 `utils-support-image-starter`（与 `TurboJpegBridge` / `TurboJpegImageEncoder`
的分工一致——桥接类放 native 模块，实现接口的类放消费方模块，避免循环依赖）：

| Java 类 | 实现接口 | `@Spi` order |
|---|---|---|
| `LibDeflateEngine` | `DeflateEngine` | 100 |
| `ImageQuantQuantizer` | `PaletteQuantizer` | 100 |

纯 Java 兜底实现注册的 order 是 `-100`，因此只要动态库加载成功就会被自动顶替，
加载失败时 `available()` 返回 `false`，`CompressionEngines` 自动回退。**功能在任何环境都可用。**

## GraalVM

`src/main/resources/META-INF/native-image/com.chua/utils-support-native-imageopt/` 下提供
`resource-config.json`（把 `native/**` 打入原生镜像）与 `native-image.properties`。
构建时需 `--enable-native-access=ALL-UNNAMED`，且注意 `NativeLoader` 的目录枚举限制，
参见父仓库 README 的「使用 Native Image 时的要求」。
