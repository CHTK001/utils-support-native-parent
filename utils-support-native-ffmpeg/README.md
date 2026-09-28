# utils-support-native-ffmpeg

FFmpeg Rust native 库。

---

## 当前状态：原生侧尚未实现，本模块不可用

**不要在本模块上开发新功能，它提供不了任何 FFmpeg 能力。**

实际内容与声明不符，事实如下：

| 项 | 实情 |
|---|---|
| 动态库导出符号 | 仅 3 个扁平 C 函数：`ffmpeg_version` / `ffmpeg_codec_available` / `ffmpeg_free_string`；**没有任何 `Java_...` JNI 符号，也没有 `JNI_OnLoad`** |
| Rust 源码 | `src/main/rust/src/lib.rs` 共 25 行，只有上述 3 个函数；`ffmpeg_codec_available` 是硬编码 `return 1` |
| Rust 依赖 | `Cargo.toml` 仅依赖 `libc`，**未引入 `ffmpeg-sys` / `ffmpeg-next`**，因此不具备做编解码/推流的能力 |
| Java 侧 | `NativeFFmpeg` 声明了 13 个 `native` 方法（推流/拉流/转码/截帧/拼接/媒体信息/旋转/水印/图片序列转视频），对应实现**在任何形态下都不存在** |

后果：`NativeFFmpeg.isLoaded()` 恒为 `false`，`getLoadError()` 返回
`UnsatisfiedLinkError`，每个方法调用都会抛该错误。上游
`utils-support-ffmpeg-rust-starter` 的 `RustFFmpegProcessor`
（`@Spi(value = {"rust", "native"}, order = 100)`）因此所有操作都会以
"stream library not loaded" 失败——失败是显式的，不会静默返回错误结果。

> 注：该模块的 Javadoc 曾声称"支持 RTMP 推流/拉流、文件转码…"，与实现不符，
> 已更正。

## 需要 FFmpeg 能力时请改用

| 场景 | 用哪个 |
|---|---|
| 通用转码、推流、截帧 | `utils-support-ffmpeg-starter`（JavaCV / Jaffree，纯 Java 封装） |
| HLS 转码 | `utils-support-native-video-processor`（Rust，已有四平台产物） |
| H.264 / H.265 / H.266 编解码 | `utils-support-native-video-codec`（Rust JNI，已有四平台产物） |

后两者是真实实现且产物齐全，不要再用本模块。

## 若要补齐本模块

需要做的事（属重大工程，非小修）：

1. `Cargo.toml` 引入 `ffmpeg-sys` 或 `ffmpeg-next`，并解决四平台 FFmpeg 静态链接；
2. 在 Rust 侧实现那 13 个操作，并补上与 `NativeFFmpeg` 一致的 JNI 层
   （符号名须为 `Java_com_chua_nativeffmpeg_support_NativeFFmpeg_<方法名>`）；
3. 参照 `.github/workflows/native-libjpeg-turbo.yml` 增补四平台交叉编译与导出符号断言。

## 快速开始（仅在你已补齐原生实现后才可用）

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-ffmpeg</artifactId>
    <version>${project.version}</version>
</dependency>
```

---

## 依赖关系

```
utils-support-native-ffmpeg
└── (无内部依赖)
```
