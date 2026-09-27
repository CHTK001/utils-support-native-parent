# utils-support-native-filesearch

跨平台快速文件搜索 Rust native 库（WizTree 能力）

实现方式：**Windows 走 NTFS MFT 直读**（`src/main/rust/src/mft.rs` 解析 `$MFT`，
直接产出路径 + 大小 + 修改时间，**需管理员权限**）；其它平台或 MFT 打不开时
回退 `walkdir`（深度上限 3）。两条路径输出**同一份 JSON ABI**，Java 侧无感知。

> 实测（管理员、C: 盘、960,768 条 MFT 记录）：
> 解析出 696,886 条有效文件记录，全量列举 **559,042** 个文件且大小/时间正确，
> 单次全盘扫描约 **20~23 秒**（逐条读取 1KB 记录 + 建路径索引；WizTree 走内存映射顺序读，
> 会更快）。设置环境变量 `CHUA_MFT_DEBUG=1` 可打印解析诊断计数。
> 非管理员时 `\\.\X:` 打不开，会自动回退到 walkdir（此时只有深度 ≤3 的结果）。

> 需要 JDK 8 运行环境时，改用同目录下的 `utils-support-native-filesearch-java8`：
> 它以 JNA 绑定**同一组**扁平 C ABI，产物可运行于 JDK 8，无需重建原生库。

---

## 快速开始

### 1. 添加依赖

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>${project.version}</version>
</dependency>
```

---

## 构建

```bash
cd src/main/rust
./build.sh auto auto release
```

---

## 原生库导出函数

源码 `src/main/rust/src/lib.rs` 实际声明 5 个 `#[no_mangle]` 导出：

| 函数 | ABI | 说明 |
|------|-----|------|
| `fast_get_version()` | `extern "C"` | 返回版本串（`1.0.0`），常量字符串不需释放 |
| `fast_search_by_name(root_dir, pattern, max_results, _callback)` | `extern "C"` | 按名称通配符搜索，返回命中条数；`_callback` 保留位，当前实现忽略 |
| `fast_search_cancel()` | `extern "C"` | 取消搜索 |
| `Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName(root_path, name_pattern, max_results)` | `extern "system"` | 返回搜索结果 JSON 字符串（`CString::into_raw`，由调用方释放） |
| `Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawGetTree(root_path, _max_depth, max_results)` | `extern "system"` | 返回结果 JSON 字符串；**`_max_depth` 当前被忽略**，内部与搜索走同一条路径（`search_to_json(root, None, max_results)`） |

两个 `Java_*_raw*` 是 JNI 名字（`_raw` 经名字改写为 `__raw`），
由 `RustFileSearchBridge` 以 FFM 直接绑定调用，不是标准 JNI 注册。

### 已移除的接口

早期 JNI 实现的 6 个符号已整体删除，源码注释说明为「曾令 JVM 崩溃」：
`getVersion`、`cancel`、`searchByName`、`getTree`、`searchBySize`、`searchByPath`。
**动态库里不应再出现这些符号**，`native-filesearch.yml` 对四条平台腿都做负向断言。

## 已提交产物状态

`src/main/resources/native/` 下按 `windows-x86_64` / `linux-x86_64` /
`darwin-x86_64` / `darwin-aarch64` 四目录组织，**四平台均已补齐**：

| 平台 | 状态 |
|------|------|
| `windows-x86_64` | `file_search.dll`（306,688 字节，sha256 `B457841B…`）**含 MFT 直读**，`rust-lld` 链接 |
| `linux-x86_64` | `libfile_search.so`（434,856 字节，sha256 `52174464…`）已按当前源码重建，**不再带 7 个已删除的 JNI 导出** |
| `darwin-x86_64` | `libfile_search.dylib`（398,160 字节，sha256 `8CBE77F5…`）已补齐 |
| `darwin-aarch64` | `libfile_search.dylib`（384,528 字节，sha256 `E1E29A19…`）已补齐 |

四平台产物均出自同一份源码，5 个导出齐全。

### 交叉编译（无需 Docker / osxcross / Apple SDK）

本模块依赖是纯 Rust（`walkdir`、`serde_json`），不链接任何 C 库，因此用
**zig 作为 C 编译器与链接器**，在 Windows 上即可直接产出 Linux 与 macOS 产物
（zig 自带 macOS 的 TBD 桩，不需要 Apple SDK）：

```bash
# 1) 准备 zig(0.15.x) 与目标标准库
#    https://ziglang.org/download/0.15.2/zig-x86_64-windows-0.15.2.zip
rustup target add x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin
cargo install cargo-zigbuild

cd src/main/rust
# 2) 交叉编译（务必清掉全局强制 rust-lld 的 CARGO_BUILD_RUSTFLAGS）
set CARGO_BUILD_RUSTFLAGS=
cargo zigbuild --release --target x86_64-unknown-linux-gnu
cargo zigbuild --release --target x86_64-apple-darwin
cargo zigbuild --release --target aarch64-apple-darwin
```

产物在 `target/<triple>/release/`，复制到 `src/main/resources/native/<平台目录>/` 即可。
Windows 本机产物用 `cargo build --release --target x86_64-pc-windows-msvc`。

> 产物校验：ELF 头 `7F 45 4C 46` 且 `e_machine=0x3E`（x86_64）；
> Mach-O 头 `0xFEEDFACF`、`cputype=0x0100000C`(ARM64) / `0x01000007`(X86_64)、`filetype=6`(MH_DYLIB)。

CI 侧仍由 `.github/workflows/native-filesearch.yml` 的四平台矩阵
（`windows-2022` / `ubuntu-22.04` / `macos-15` × 2）构建，产物回填到分支
`build/filesearch-native-artifacts` 后合并回 `main`。上述交叉编译产物**未经真机运行验证**，
正式出货建议以 CI 产物为准。
