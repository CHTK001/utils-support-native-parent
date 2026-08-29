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

## 被谁使用

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-nmap</artifactId>
    <version>${project.version}</version>
</dependency>
```

调用方: `utils-support-network-parent` 网络扫描模块
