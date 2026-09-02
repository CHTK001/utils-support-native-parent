# Linux x86_64 编译指南

## 环境要求

- Ubuntu 22.04+ (推荐)
- `x86_64-linux-gnu-gcc` (交叉编译工具链)
- Rust 1.75+ (via rustup)
- 至少 8GB RAM

## 一键编译

```bash
# 在 Linux 服务器上执行
cd ~/native-build/sources
bash build-linux.sh
```

## 各模块编译命令

### Rust 模块

每个模块在服务器上的源码路径：`~/native-build/sources/utils-support-native-{module}/src`

`.cargo/config.toml` 已配置 cross-compile，直接执行：

```bash
cd ~/native-build/sources/utils-support-native-{module}/src
cargo build --target x86_64-unknown-linux-gnu --release
```

生成的 `.so` 文件：

| 模块 | Cargo lib name | .so 文件名 |
|------|---------------|-----------|
| video-codec | chua-native-video-codec | libchua_native_video_codec.so |
| nmap | rust_nmap | librust_nmap.so |
| datarecovery | data_recovery_ffi | libdata_recovery_ffi.so |
| filesearch | rust_filesearch | libfile_search.so |
| headless | headless-rust | libheadless_rust.so |
| filestorage | rust_filestorage_processor | libfile_storage.so |
| smb | rust_smb_server | librust_smb_server.so |
| ffmpeg | ffmpeg_rust | libffmpeg_rust.so |
| metrics | metrics-native | libmetrics_native.so |
| video-processor | video_processor | libvideo_processor.so |

生成的 `.so` 复制到对应模块的 `src/main/resources/native/linux-x86_64/` 目录。

### SQLite C 模块

```bash
cd utils-support-native-sqlite/src/main/c
gcc -shared -fPIC -O2 -o libsqlite3_hook.so sqlite3_hook.c \
    -I. -Wl,-soname,libsqlite3_hook.so -Wl,-rpath,\$ORIGIN
cp libsqlite3_hook.so ../../resources/native/linux-x86_64/
```

## .cargo/config.toml 配置

每个 Rust 模块的 `.cargo/config.toml`：

```toml
[build]
target = "x86_64-unknown-linux-gnu"

[target.x86_64-unknown-linux-gnu]
linker = "x86_64-linux-gnu-gcc"
rustflags = ["-C", "link-arg=-Wl,-rpath,$ORIGIN"]
```

## 已验证结果

所有 10 个 Rust 模块 + 1 个 SQLite C 模块均已在 Linux 服务器上成功编译。
共生成 11 个 `.so` 文件，位于各模块的 `src/main/resources/native/linux-x86_64/`。

## 依赖说明

- SQLite C 模块需要 `-ldl -lpthread` 链接（系统自带）
- Rust 模块通过 cargo 自动链接系统库
- `libdl.so`, `libm.so`, `libpthread.so` 等均为 Ubuntu 标准系统库
