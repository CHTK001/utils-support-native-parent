# utils-support-native-parent

预编译 native 二进制库（Rust cdylib）的 Maven 聚合项目，独立于 Java 代码分发。所有 native 库通过 Rust 编写并编译为平台相关动态链接库，以 jar 形式发布到 GitHub Packages。

## 模块

| 模块 | 说明 |
|---|---|
| `utils-support-native-rust-proxy` | 代理协议 native 库（HTTP / SOCKS5 / RDP / SSH / VNC / FTP） |
| `utils-support-native-perf` | 性能计数器 native 库 |
| `utils-support-native-nmap` | Nmap 集成 native 库 |
| `utils-support-native-ffmpeg` | FFmpeg RTMP native 库 |
| `utils-support-native-filesystem` | 文件系统压缩/解压 native 库 |

## 使用方式

### Maven

```xml
<repositories>
    <repository>
        <id>github</id>
        <url>https://maven.pkg.github.com/CHTK001/utils-support-native-parent</url>
    </repository>
</repositories>

<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-xxx</artifactId>
    <version>4.0.0.41</version>
</dependency>
```

### Gradle

```kotlin
repositories {
    maven {
        url = uri("https://maven.pkg.github.com/CHTK001/utils-support-native-parent")
    }
}

dependencies {
    implementation("com.chua:utils-support-native-xxx:4.0.0.41")
}
```

## 构建

```bash
mvn clean install
```

## 发布

```bash
mvn deploy
```

目标仓库：`https://maven.pkg.github.com/CHTK001/utils-support-native-parent`
