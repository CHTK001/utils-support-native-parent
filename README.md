# utils-support-native-parent

预编译 native 二进制库（Rust cdylib）的 Maven 聚合项目，独立于 Java 代码分发。所有 native 库通过 Rust 编写并编译为平台相关动态链接库，以 jar 形式发布到 GitHub Packages。

## 模块

| 模块 | 说明 | 被以下模块使用 |
|---|---|---|
| `utils-support-native-rust-proxy` | 代理协议 native 库（HTTP / SOCKS5 / RDP / SSH / VNC / FTP） | `utils-support-native-video-codec` |
| `utils-support-native-perf` | 性能计数器 native 库 | — |
| `utils-support-native-nmap` | Nmap 集成 native 库 | — |
| `utils-support-native-ffmpeg` | FFmpeg RTMP native 库 | `utils-support-ffmpeg-rust-starter` |
| `utils-support-native-sqlite` | SQLite update_hook 原生动态库（环形缓冲 + JSON 事件） | — |
| `utils-support-native-datarecovery` | 数据恢复 Rust native 库 | — |
| `utils-support-native-video-codec` | H.264/H.265/H.266 编解码（JNI，含预编译 `.dll/.so/.dylib`） | `utils-support-ffmpeg-rust-starter`、`utils-support-example-starter` |
| `utils-support-native-video-processor` | Video HLS 转码 Rust native 库 | `utils-support-video-processor-starter` |
| `utils-support-native-filestorage` | 文件存储 Rust native 库（高性能 URL 参数解析 + 图片滤镜 + HEIC/HEIF 预览转码） | `utils-support-filestorage-starter` |
| `utils-support-native-filesearch` | 文件搜索 Rust native 库（WizTree 能力，跨平台） | — |
| `utils-support-native-smb` | SMB2/3 服务端 Rust native 库（smb-server crate） | `utils-support-smb-starter` |
| `utils-support-native-metrics` | 系统指标 Rust native 库 | `utils-support-metrics-starter` |
| `utils-support-native-cuda` | CUDA 运行时库（cudart/cublas/cudnn）环境检测与自动安装 | `utils-support-common-starter` |
| `utils-support-native-headless` | 无头环境 native 支持 | — |

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
- `reachability-metadata.json`（`foreign.downcalls`）：`utils-support-native-shm-queue` / `shm-queue-http` 的 FFM downcall 签名（`void*`/`jint`/`jlong`/`jshort`），原生镜像中执行 FFM 调用必需；
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

## 测试覆盖

> 最新测试结果：2026-09-01，运行 `com.chua.test.NativeTestSuite`

| 模块 | 测试状态 | 说明 |
|------|---------|------|
| video-codec | ✅ 5/5 | h264/h265/h266 编码、H.264 解码、getVersion |
| datarecovery | ✅ 2/2 | scan (142 files), permanentDelete |
| filesearch | ✅ 2/2 | searchByName, getTree |
| metrics | ✅ 2/2 | poll (4396 chars), start/stop cycle |
| ffmpeg | ✅ 1/1 | h264Encode via bridge (348 bytes) |
| video-processor | ✅ 2/2 | isAvailable=true, version=1.0.0 |
| smb | ✅ 2/2 | dllLoaded + smb_start (port 1445) |
| sqlite | ✅ 2/2 | DLL load success |
| nmap | ✅ 5/5 | getVersion, isValidIp, port scan, resolve |
| headless | ⬜ 未测 | DLL 存在但为 C-style exports，无 JNI 符号 |
| cuda | ⬜ 未测 | 无 DLL，仅 CUDA 环境检测脚本 |
| filestorage | ⬜ 未测 | DLL 存在但为 C-style exports，无 JNI 符号 |

**已测：23/23 PASS · 待桥接：headless、filestorage · 环境检测：cuda**

详细测试输出：`test-output/native_test_suite.txt`

## 发布

```bash
mvn deploy
```

目标仓库：`https://maven.pkg.github.com/CHTK001/utils-support-native-parent`
