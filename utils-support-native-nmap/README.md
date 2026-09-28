# utils-support-native-nmap

高性能网络端口扫描库，支持 TCP/UDP 扫描、DNS 解析、服务识别。

## 功能场景

- 内网主机/端口探测
- 网络资产发现
- 安全扫描

## 构建

```bash
cd src/main/rust
./build.sh auto auto release
# Windows: rust_nmap.dll
# Linux:   librust_nmap.so
```

Linux 构建: `docker run --rm -v $(pwd):/src rust:latest bash -c "cd /src && ./build.sh linux x86_64 release"`

## 已提交产物状态

`src/main/resources/native/` 下按 `windows-x86_64` / `linux-x86_64` /
`darwin-x86_64` / `darwin-aarch64` 四目录组织，但**当前只有两个平台有产物**：

| 平台 | 状态 |
|------|------|
| `windows-x86_64` | `rust_nmap.dll` 已与当前源码一致（sha256 校验一致），18 个 JNI 导出齐全 |
| `linux-x86_64` | `librust_nmap.so` **构建自损坏源码，缺 5 个导出**：`detectService`、`detectOs`、`resolveHostname`、`isValidSubnet`、`getVersion` |
| `darwin-x86_64` | **产物缺失** |
| `darwin-aarch64` | **产物缺失** |

Linux 那个 `.so` 里还残留 `scanSubnet0EB2r_EB2R_`、`scanSubnet0EEB2u_` 这类
畸形符号名——正是 `#[no_mangle] pub unsafe extern "system"` 的属性被吞、
Rust 改用 v0 名字改写所产生的特征。该缺陷已按 `_` 分隔的属性写法修复，
但已提交的 `.so` 仍是修复前的构建，**Linux 上这 5 个接口不可用**。

补齐方式：`.github/workflows/native-nmap.yml` 覆盖四条腿
（`windows-2022` / `ubuntu-22.04` / `macos-15` × 2），对 18 个符号逐个断言，
产物回填到分支 `build/nmap-native-artifacts` 后合并回 `main`。
本地无法补：Linux 交叉编译需要 `x86_64-linux-gnu-gcc`，
macOS 需要 Apple SDK 与 osxcross。

## 被谁使用

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-nmap</artifactId>
    <version>${project.version}</version>
</dependency>
```

调用方: `utils-support-network-parent` 网络扫描模块
