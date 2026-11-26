# utils-support-native-anydoc

`firecrawl/anydoc`（v0.2.5，Rust）的 C-ABI 动态库封装与 Java FFM 绑定。

anydoc 把 **14 种文档**解析为 GitHub-Flavored Markdown：
doc / docx / docm / odt / pdf / ppt / pptx / pps / pot / rtf / epub /
xlsx / xlsm / xlsb / xls / ods / odp / csv。
纯文档解析：**无 OCR、无模型推理**（扫描件 PDF 返回 `NeedsOcr` 类错误）。
官方只有 PyO3 / NAPI / WASM 绑定；本模块补齐通用 C-ABI，供
Java FFM / Python ctypes / C / C# 等原生消费。

## 模块结构

```text
utils-support-native-anydoc/
├─ pom.xml                                  # Java 25，FFM 绑定层
├─ src/main/java/com/chua/anydoc/support/AnydocBridge.java
├─ src/main/rust/                           # Rust cdylib 源码（anydoc_ffi）
│  ├─ Cargo.toml                            # 依赖 crates.io anydoc = "0.2.5"
│  ├─ Cargo.lock                            # 入库，保证 CI 产物可复现
│  ├─ smoke.c                               # Linux/CI 用 C-ABI 冒烟探针
│  ├─ Dockerfile                            # Linux 构建即冒烟（rust:slim）
│  └─ src/lib.rs
└─ src/main/resources/native/
   ├─ windows-x86_64/anydoc_ffi.dll
   ├─ linux-x86_64/libanydoc_ffi.so
   ├─ darwin-x86_64/libanydoc_ffi.dylib     # 由 native-anydoc workflow 产出
   └─ darwin-aarch64/libanydoc_ffi.dylib    # 由 native-anydoc workflow 产出
```

## C ABI 符号

| 符号 | 签名 | 说明 |
|---|---|---|
| `anydoc_version` | `char* (void)` | 版本串（**静态存储，禁止 free**） |
| `anydoc_convert_file` | `char* (const char* path)` | 按路径转换；内容签名探测、扩展名兜底 |
| `anydoc_convert_bytes` | `char* (const uint8_t* data, size_t len, const char* format)` | 内存转换；`format` 传 NULL 自动探测，或传扩展名（CSV 无签名必须显式命名） |
| `anydoc_last_error` | `const char* (void)` | 当前线程最近一次错误（**不得 free**；下一次 anydoc 调用前有效） |
| `anydoc_free_string` | `void (char* s)` | 只用于释放 convert 返回的串 |

返回约定：成功返回 NUL 结尾 UTF-8 C 串（调用方用 `anydoc_free_string`
释放）；失败返回 NULL，此时用 `anydoc_last_error()` 取错误。
所有导出内部 `catch_unwind`，Rust panic 不会跨 FFI 边界。

## Java 用法（JDK 25 FFM）

```java
// JVM 需加 --enable-native-access=ALL-UNNAMED
if (AnydocBridge.isAvailable()) {
    String version = AnydocBridge.version();
    byte[] docx = Files.readAllBytes(Path.of("contract.docx"));
    String markdown = AnydocBridge.convertBytes(docx, null);      // 自动探测
    String csvMd   = AnydocBridge.convertBytes(csvBytes, "csv");  // 无签名格式显式命名
    String fileMd  = AnydocBridge.convertFile(Path.of("slide.pptx"));
}
```

内存管理（谁分配谁释放）全部封装在 `AnydocBridge` 内：convert 结果由
桥接层调用 `anydoc_free_string` 归还；`version` / `last_error` 属静态 /
线程局部存储，绝不 free。

## 构建与验收

```bash
# Rust 侧
cd utils-support-native-anydoc/src/main/rust
cargo build --release            # 产物 target/<triple>/release/{lib}anydoc_ffi.{dll,so,dylib}

# Linux 构建即验收（Docker，仓库根目录执行；冒烟不过则构建失败）
docker build -t anydoc-ffi:linux-x86_64 \
  -f utils-support-native-anydoc/src/main/rust/Dockerfile \
  utils-support-native-anydoc/src/main/rust
```

- `native-anydoc` workflow（GitHub Actions）：四平台交叉编译 +
  导出符号断言 + 架构校验 + ctypes 运行时冒烟（dlopen → version →
  RTF 转换 → 错误路径），产物回填分支 `build/anydoc-native-artifacts`。
- **`darwin-x86_64` 的冒烟按矩阵 `smoke: false` 跳过，属设计内**：
  GitHub 的 `macos-15` 是 arm64 runner，x86_64 dylib 在上面无法 dlopen
  （架构不匹配），该腿只做构建 + 架构校验 + 导出符号断言。其运行时正确性
  由 `darwin-aarch64`（同源码同工具链）与 linux/windows 两腿的冒烟共同覆盖。
- 本机（Windows x86_64）已按同一 C-ABI 用 JDK 25 FFM 探针完成 6/6 断言
  （version / RTF 字节自动探测 / CSV 显式命名 / 文件路径转换 /
  垃圾输入报错 / 非法格式名报错）。

## 已知边界

- **不支持 OCR**：扫描件 PDF / 图片型文档需先走云 OCR 或本地 OCR 引擎。
- CSV 没有内容签名，`convert_bytes` 必须显式传 `format="csv"`。
- 错误信息按线程隔离（Rust thread_local），跨线程不可见。
- 上层消费：文档 → Markdown 属 RAG 预处理等调用方的编排，本模块不含业务规则。
