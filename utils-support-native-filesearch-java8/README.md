# utils-support-native-filesearch-java8

`file_search` 原生库的 **Java 8 + JNA** 绑定，与 Java 25 的 Panama FFM 主模块
（`utils-support-native-filesearch`）能力对齐，供只能运行在 JDK 8 的环境使用。

---

## 为什么单独一个模块

仓库现有 `com.chua.filesearch.support.bridge.RustFileSearchBridge` 依赖
`java.lang.foreign`（Panama FFM，JDK 22+）与 `record`，产物为 Java 25 字节码；
其依赖的 `utils-support-common-starter` 同样是 `release 25`。因此它**无法**在
JDK 8 上加载。

本模块因此自包含：只依赖 JNA + SLF4J，不依赖任何 release 25 组件，编译目标为
`release 8`（字节码 52.0），可直接在 JDK 8 上运行。

| 维度 | 主模块（`-filesearch`） | 本模块（`-filesearch-java8`） |
|------|------------------------|------------------------------|
| 绑定方式 | Panama FFM | JNA |
| 运行 JDK | 25+ | 8+ |
| 结果类型 | `record FileResultData` | `class FileSearchResult` |
| JSON 解析 | `Json5`（common-starter） | 内置 `SearchJsonParser` |
| 原生库 | 复用同一组四平台产物 | 复用同一组四平台产物 |

两套绑定调用的是**同一组扁平 C ABI 符号**（`fast_get_version`、
`fast_search_cancel`、`Java_..._RustFileSearchBridge__rawSearchByName`、
`Java_..._RustFileSearchBridge__rawGetTree`），无需重建原生库。

---

## 快速开始

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch-java8</artifactId>
    <version>${project.version}</version>
</dependency>
```

```java
import com.chua.filesearch.support.bridge.jna.FileSearchResult;
import com.chua.filesearch.support.bridge.jna.JnaFileSearchBridge;

JnaFileSearchBridge.loadLibrary();
if (JnaFileSearchBridge.isLoaded()) {
    int count = JnaFileSearchBridge.searchByName("D:/data", "*.log", 100,
            result -> System.out.println(result.getPath() + "  " + result.getSize()));
    System.out.println("命中 " + count + " 个文件，版本 " + JnaFileSearchBridge.getVersion());
}
```

### 加载原生库

`loadLibrary()` 按 `os.name` / `os.arch` 从 jar 内
`/native/{windows|linux|darwin}-{x86_64|aarch64}/` 抽取动态库到
`java.io.tmpdir/chua-native-file-search-java8/` 再交给 JNA 加载。

若 classpath 中缺产物，可用系统属性覆盖：

```bash
java -Dchua.filesearch.native.path=/abs/path/libfile_search.so ...
```

### API

| 方法 | 说明 |
|------|------|
| `loadLibrary()` / `isLoaded()` / `getLoadError()` / `getLibraryPath()` | 加载与诊断 |
| `getVersion()` | 原生库版本（读取失败返回空串） |
| `cancel()` | 取消搜索（当前原生实现为空操作） |
| `searchByName(root, pattern, max, cb)` | 按名称 glob 搜索 |
| `getTree(root, depth, max, cb)` | 遍历目录（depth 不生效） |
| `searchBySize(root, min, max, max, cb)` | 按大小过滤（Java 侧实现） |
| `searchByPath(root, pattern, max, cb)` | 按路径 glob 过滤（Java 侧实现） |
| `searchByNameSafe(...)` / `getTreeSafe(...)` | 先 `loadLibrary()` 再调用的安全封装 |

搜索类方法返回命中条数；原生调用失败返回 `-1`，未加载时抛
`IllegalStateException`。回调可选，传 `null` 即只统计数量。

---

## 原生侧限制（与 FFM 版一致）

- 遍历深度在原生侧硬编码为 3，`getTree` 的 `depth` 参数不生效；
- 原生遍历跳过目录项，故 `FileSearchResult#isDirectory()` 恒为 `false`，
  `getTree` 实际返回文件列表；
- 原生 JSON 仅含 path / size / modified / ext 四项，其余字段为默认值；
- `searchBySize` / `searchByPath` 原生无对应能力，由本类在全量结果上做 Java 侧过滤；
- 原生返回的 JSON 指针由 Rust 分配，跨分配器释放不安全，本模块与 FFM 版一致
  不释放（每次搜索泄漏一个小字符串）。
- 原生 `Java_*` 两个符号用 `extern "system"` 导出；已提交产物均为 64 位，
  该调用约定与 JNA 默认的 `cdecl` 在 64 位上一致。32 位平台不受支持。

---

## 构建

```bash
mvn -pl utils-support-native-filesearch-java8 -am package
```

`native/` 资源在构建时从 `../utils-support-native-filesearch/src/main/resources/native`
复制而来，**不在本模块重复入库**，因此产物始终与主模块的四平台二进制保持一致。

## 冒烟测试

```bash
# Windows（; 分隔），Linux/macOS 用 : 分隔
javac -encoding UTF-8 -cp "target/classes;.../jna-5.14.0.jar" \
  -d target/smoke src/smoke/java/FilesearchJnaBridgeSmoke.java
java -cp "target/classes;target/smoke;jna-5.14.0.jar;slf4j-api-1.7.36.jar" \
  FilesearchJnaBridgeSmoke
```

冒烟会校验名称搜索、目录遍历、按大小 / 路径过滤与版本号，无需项目其它模块。
