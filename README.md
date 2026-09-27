# utils-support-native-parent

预编译 native 二进制库（Rust cdylib）的 Maven 聚合项目，独立于 Java 代码分发。所有 native 库通过 Rust 编写并编译为平台相关动态链接库，以 jar 形式发布到 GitHub Packages。

## 模块

| 模块 | 说明 | JDK | 被以下模块声明依赖 |
|---|---|---|---|
| `utils-support-native-cuda` | CUDA 运行时库（cudart/cublas/cudnn）环境检测与自动安装 | 1.8 | 无 |
| `utils-support-native-datarecovery` | 数据恢复 Rust native 库（JNI） | 1.8 | `utils-support-datarecovery-starter` |
| `utils-support-native-ffmpeg` | FFmpeg RTMP native 库（JNI，`RegisterNatives`） | 1.8 | `utils-support-ffmpeg-rust-starter` |
| `utils-support-native-filesearch` | 文件搜索 Rust native 库（WizTree 能力，JNI） | **25** | `utils-support-filesearch-starter` |
| `utils-support-native-filesearch-java8` | 文件搜索 Rust native 库的 Java 8 JNA 绑定（同一组扁平 C ABI，独立于 25 档主模块） | 1.8 | 无 |
| `utils-support-native-fastfilesearch` | Windows NTFS MFT 直读文件搜索（预编译 `fast_file_search`，需管理员权限） | **25** | `utils-support-syslog-starter` |
| `utils-support-native-fastfilesearch-java8` | 同上 MFT 库的 Java 8 JNA 绑定（同一份 `fast_file_search.dll`） | 1.8 | 无 |
| `utils-support-native-filestorage` | 文件存储 Rust native 库（URL 参数解析 + 图片滤镜 + HEIC 预览转码） | 1.8 | 无 |
| `utils-support-native-headless` | 无头环境 native 支持 | 1.8 | 无 |
| `utils-support-native-libjpeg-turbo` | libjpeg-turbo TurboJPEG（SIMD JPEG 编解码，FFM） | **25** | `utils-support-image-starter` |
| `utils-support-native-metrics` | 系统指标 Rust native 库（FFM） | **25** | `utils-support-metrics-starter` |
| `utils-support-native-needle` | Needle 工具调用 / 结构化抽取引擎（cactus-compute，FFM） | **25** | `utils-support-deeplearning-needle-starter` |
| `utils-support-native-nmap` | Nmap 集成 native 库（JNI） | 1.8 | `utils-support-nmap-starter` |
| `utils-support-native-smb` | SMB2/3 服务端 Rust native 库（smb-server crate，FFM） | **25** | `utils-support-smb-starter` |
| `utils-support-native-sqlite` | SQLite update_hook 原生动态库（环形缓冲 + JSON 事件） | 1.8 | `utils-support-sqlite-starter`、`spring-api-support-system-starter` |
| `utils-support-native-uia` | Windows UI 自动化原语（FFM） | **25** | `utils-support-native-wechat` |
| `utils-support-native-video-codec` | H.264/H.265/H.266 编解码（JNI） | 1.8 | `utils-support-example-starter` |
| `utils-support-native-video-processor` | Video HLS 转码 Rust native 库（JNI） | 1.8 | `utils-support-video-processor-starter` |
| `utils-support-native-wechat` | 微信 4.x WCDB 原生读取（JNI + FFM） | **25** | 无 |

「无」表示当前没有任何 pom 声明依赖它（据全仓 pom 扫描）；这些模块仍可被应用直接引用，
不代表已废弃。

## 编译级别与运行时要求

父 POM 未配置 `maven.compiler.release`，默认落到 `target 1.8`。整个仓库只维护两档：

| 档位 | 适用 | 模块 |
|---|---|---|
| **1.8**（父 POM 默认） | 纯 Java / 传统 JNI（`native` 方法）/ JNA 绑定，无新语法 | cuda、datarecovery、fastfilesearch-java8、ffmpeg、filesearch-java8、filestorage、headless、nmap、sqlite、video-codec、video-processor |
| **25**（模块内显式配置） | 使用 `java.lang.foreign`（FFM，JDK 22+）或 `record`，或需 `--enable-preview` | fastfilesearch、filesearch、libjpeg-turbo、metrics、needle、smb、uia、wechat |

**引用 25 档模块的应用运行时必须是 JDK 25**（CI 亦为 temurin 25）；1.8 档模块可运行于更早的 JDK。
新增 native 模块时请按此二选一，不要引入第三档。

两个细节：

- `native-filesearch` 不含 FFM，但它用到 `record`，父 POM 默认的 1.8 会报「-source 8 中不支持 记录」，故仍归 25 档。
- `native-needle` 使用 FFM 的**最低**要求是 22，这里取 25 仅为与同档模块一致、便于审计。


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

### Native 库交叉编译

预编译动态库位于各模块 `src/main/resources/native/<platform>/`，可分别按平台交叉编译：

| 平台 | 平台目录 | 构建脚本 | 指南 |
|------|----------|----------|------|
| Linux x86_64 | `linux-x86_64` | `build-linux.sh` | [BUILD-LINUX.md](BUILD-LINUX.md) |
| macOS (osxcross) | `darwin-aarch64` / `darwin-x86_64` | `build-macos.sh` | [BUILD-MACOS.md](BUILD-MACOS.md) |

macOS 示例（需 Linux 主机 + osxcross）：

```bash
export OSXCROSS_ROOT=/opt/osxcross
./build-macos.sh aarch64 x86_64   # Apple Silicon + Intel 双架构 dylib
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

### Java 8 JNA 绑定冒烟

`utils-support-native-filesearch-java8` 的 `FilesearchJnaBridgeSmoke` 只依赖本模块产物与
`jna-5.14.0.jar`，已在 Temurin **1.8.0_504** 上验证通过：库加载、`getVersion`、
`searchByName`、`getTree`、`searchBySize`、`searchByPath` 全部通过（`SMOKE OK`）。

```bash
javac --release 8 -encoding UTF-8 -cp "target/classes;.../jna-5.14.0.jar" \
  -d target/smoke src/smoke/java/FilesearchJnaBridgeSmoke.java
java -cp "target/classes;target/smoke;jna-5.14.0.jar;slf4j-api-1.7.36.jar" FilesearchJnaBridgeSmoke
```

## 发布

```bash
mvn deploy
```

目标仓库：`https://maven.pkg.github.com/CHTK001/utils-support-native-parent`
