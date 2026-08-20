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

## GraalVM 支持

本项目为 GraalVM Native Image 提供了开箱即用的元数据（位于各模块 `src/main/resources/META-INF/native-image/com.chua/<module>/`）：

- `resource-config.json`：将 `native/**` 平台动态库与 `META-INF/services/**` SPI 声明打入原生镜像；
- `jni-config.json`：`utils-support-native-video-codec` 的 JNI 类（`NativeVideoCodec`）及其全部 native 方法签名；
- `native-image.properties`：合并到最终原生镜像构建的推荐参数。

### 使用 Native Image 时的要求

1. **动态库可达**：`utils-support-native-video-codec` 通过 `System.loadLibrary("chua_native_video_codec")` 加载，
   构建原生镜像后需保证 `chua_native_video_codec.dll/.so/.dylib` 位于运行期 `java.library.path`；
2. **Foreign API**：`utils-support-native-shm-queue`、`utils-support-native-shm-queue-http` 使用 Panama FFM，
   Native Image 需启用原生访问：

   ```bash
   native-image --enable-native-access=ALL-UNNAMED -jar app.jar
   ```

   或 Maven 场景：`mvn -Pnative native:compile -Dnative.buildtools.build-args="--enable-native-access=ALL-UNNAMED"`

3. **NativeLoader 目录枚举限制**：`NativeLoader`/`NativeUtils` 依赖 `ClassLoader.getResources(native/<platform>)`
   做目录枚举，该方式在原生镜像中不可用；如需在 Native Image 下加载 classpath 内动态库，请改用
   `NativeUtils.load(libName, null)` 精确加载，并在应用侧运行 `native-image` tracing agent 补充反射元数据。

### 构建

```bash
mvn clean install
```

## 发布

```bash
mvn deploy
```

目标仓库：`https://maven.pkg.github.com/CHTK001/utils-support-native-parent`
