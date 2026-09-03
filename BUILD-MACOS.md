# macOS (osxcross) 交叉编译指南

在 Linux x86_64 主机上使用 osxcross 交叉编译 macOS `.dylib`（Apple Silicon 与 Intel 双架构）。

## 环境要求

- Ubuntu 22.04+（推荐）
- [osxcross](https://github.com/tpoechtrager/osxcross)（含 macOS SDK + clang 工具链）
  默认安装到 `/opt/osxcross`，可用 `OSXCROSS_ROOT` 覆盖
- Rust 1.75+（via rustup）
- 至少 8GB RAM

## 安装 osxcross

```bash
# 1. 克隆并编译 osxcross（需要 macOS SDK 压缩包放置于 incoming/）
git clone https://github.com/tpoechtrager/osxcross.git /opt/osxcross
cd /opt/osxcross
# 将 MacOSX11.3.sdk.tar.xz 放入 incoming/ 后执行
./build.sh
```

安装完成后：

- `x86_64-apple-darwin-clang` — Intel 交叉编译器
- `aarch64-apple-darwin-clang` — Apple Silicon 交叉编译器
- `x86_64-apple-darwin-ar` / `aarch64-apple-darwin-ar` — 静态库工具
- 均位于 `/opt/osxcross/bin`

## 一键编译

```bash
cd ~/native-build/sources
chmod +x build-macos.sh

./build-macos.sh            # 默认 aarch64 (Apple Silicon)
./build-macos.sh aarch64    # Apple Silicon
./build-macos.sh x86_64     # Intel Mac
./build-macos.sh aarch64 x86_64   # 双架构
```

如果 osxcross 不在 `/opt/osxcross`：

```bash
export OSXCROSS_ROOT=/path/to/osxcross
./build-macos.sh
```

## 脚本行为

`build-macos.sh` 会自动：

1. 将 `$OSXCROSS_ROOT/bin` 加入 `PATH`；
2. 为每个目标架构 `rustup target add <aarch64|x86_64>-apple-darwin`；
3. 通过 `CARGO_TARGET_*_APPLE_DARWIN_LINKER` 与 `CC*_apple_darwin` 注入 osxcross clang；
4. 编译全部 Rust 模块 cdylib 并复制到各模块 `src/main/resources/native/darwin-{arch}/`；
5. 使用 `aarch64-apple-darwin-clang`/`x86_64-apple-darwin-clang` 编译
   `utils-support-native-sqlite` 的 macOS 专用 C 源码（`sqlite3_hook_macos.c`）。

生成的平台目录（与 `NativeUtils.getPlatformDir()` 一致）：

| 架构 | Rust target | 平台目录 |
|------|-------------|----------|
| Apple Silicon | aarch64-apple-darwin | `darwin-aarch64` |
| Intel | x86_64-apple-darwin | `darwin-x86_64` |

## 各模块 dylib 产物

| 模块 | Cargo lib name | .dylib 文件名 |
|------|---------------|--------------|
| video-codec | chua-native-video-codec | libchua_native_video_codec.dylib |
| nmap | rust_nmap | librust_nmap.dylib |
| datarecovery | data_recovery_ffi | libdata_recovery_ffi.dylib |
| video-processor | video_processor | libvideo_processor.dylib |
| filesearch | rust_filesearch | libfile_search.dylib |
| headless | headless-rust | libheadless_rust.dylib |
| filestorage | rust_filestorage_processor | libfile_storage.dylib |
| smb | rust_smb_server | librust_smb_server.dylib |
| ffmpeg | ffmpeg_rust | libffmpeg_rust.dylib |
| metrics | metrics-native | libmetrics_native.dylib |
| sqlite | — (C 源码) | libsqlite3_hook.dylib |

## SQLite 说明

macOS 没有 io_uring，因此 `build-macos.sh` 编译的是
`utils-support-native-sqlite/src/main/c/sqlite3_hook_macos.c`
（POSIX pipe + select + pthread 线程模型，提供同步与异步回调 API）。
运行时通过 `dlopen` 动态加载系统 `libsqlite3.dylib`，无需内置 SQLite。

## JNI 头文件

JNI 模块（video-codec、nmap、datarecovery、video-processor）使用
`jni-headers/darwin/`（通过 `-Isysroot` 由 osxcross SDK 提供亦可）。
macOS 的 `jni_md.h` 与 Linux 几乎一致（LP64，`long jint`）。

## 验证

在 macOS 上验证：

```bash
# 检查符号（Apple Silicon）
nm -gU libchua_native_video_codec.dylib | grep Java_
# 检查架构
file libmetrics_native.dylib        # arm64 / x86_64
lipo -info libmetrics_native.dylib
```

Java 侧通过 `NativeUtils`/`NativeLoader` 从 `classpath:/native/darwin-{arch}/` 自动加载，
无需额外配置。

## 已验证结果

目标：在 Intel Linux 主机上用 osxcross 交叉编译 Apple Silicon（aarch64）dylib。
全部 11 个模块（10 Rust + 1 C）均须通过 `file` 校验为 macOS arm64 二进制的公平判断。

## 故障排查

| 问题 | 处理 |
|------|------|
| `osxcross not found` | 确认 `$OSXCROSS_ROOT` 指向 osxcross 根目录 |
| `linker 'aarch64-apple-darwin-clang' not found` | 确认 osxcross 编译完成且 bin 在 PATH |
| `cc` crate 找不到编译器 | 检查脚本已导出 `CC_<target>_apple_darwin` |
| JNI 头文件缺失 | 使用 SDK 自带头，或设 `JAVA_HOME` 指向含 `include/` 的 JDK |