# utils-support-native-libjpeg-turbo

libjpeg-turbo 3.1.2 原生动态库。
        以 Rust cdylib 薄封装 TurboJPEG 传统 C API，静态链接 libjpeg-turbo（含 SIMD 汇编核），
        导出扁平 C ABI 供 Java Panama FFM 侧调用。

---

## 快速开始

### 1. 添加依赖

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-libjpeg-turbo</artifactId>
    <version>${project.version}</version>
</dependency>
```

### 2. 加载与调用

动态库随 jar 打包在 `native/{platform}/chua_native_turbojpeg.*`，由消费侧
（`utils-support-image-starter` 的 `TurboJpegBridge`）通过 `NativeLoader` 抽取加载，
无需手动 `System.loadLibrary`。

---

## 导出符号

| 符号 | 说明 |
| --- | --- |
| `chua_tj_version` | 返回内嵌的 libjpeg-turbo 版本串 |
| `chua_tj_last_error` | 返回当前线程最近一次错误描述 |
| `chua_tj_compress` | RGB/BGR/GRAY 打包像素 → JPEG 字节 |
| `chua_tj_probe` | 仅解析 JPEG 头，返回宽高与色度二次采样 |
| `chua_tj_decompress` | JPEG 字节 → 打包像素（含行跨距） |
| `chua_tj_free` | 释放由 `tjAlloc` 分配、经上述出参返回的缓冲区 |

---

## 重新构建

Windows（需要 cargo、cmake、ninja、nasm 与任一 C 编译器）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File src/main/rust/build.ps1 -Version 3.1.2 -Platform windows-x86_64
```

Linux / macOS（位置参数为 `<os> <arch>`，可用 `auto` 让脚本自行探测）：

```bash
bash src/main/rust/build.sh linux x86_64
bash src/main/rust/build.sh darwin aarch64
```

脚本会下载并 CMake 静态构建上游 libjpeg-turbo，再 `cargo build --release`，
最后把产物复制到 `src/main/resources/native/<platform>/`。
若已自行构建好静态库，直接指定库目录即可只编译 Rust 门面：

```bash
TURBOJPEG_LIB_DIR=/path/to/libjpeg-turbo-3.1.2/build \
  cargo build --release --manifest-path src/main/rust/Cargo.toml
```

---

## 依赖关系

```
utils-support-native-libjpeg-turbo
└── (无内部依赖，原生侧静态链接 libjpeg-turbo 3.1.2)
```

## 注意事项

- 产物为自包含动态库：Windows 侧仅依赖 `KERNEL32.dll` 与 UCRT，不附带 MinGW 运行时。
- GraalVM Native Image 场景需用 tracing agent 采集实际加载路径后再补充元数据，
  本模块仅预置 `resource-config.json`（放行 `native/.*`）。
