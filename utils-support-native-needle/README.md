# utils-support-native-needle

Needle 推理引擎接入模块。

## ⚠️ 当前状态：未就绪，不要在生产路径上调用

本模块**只有 Java 门面，没有原生库，也没有任何 FFM 绑定**。具体缺三样：

| 缺失项 | 现状 |
|--------|------|
| `chua_native_needle` 二进制 | 不在本仓库，全项目搜不到该文件的任何实体 |
| 原生源码（Rust / C / C++） | 本模块无 Cargo.toml、无 C/C++ 源文件 |
| FFM 符号绑定 | `NeedleNative` 不声明任何 `MethodHandle` |

因此即使把动态库补齐，当前门面也**无法真正推理**——`init` / `complete` 会抛
`UnsupportedOperationException`。这是有意为之：宁可显式失败，也不要返回空串
让上层把"空回复"误当成推理结果。

## 为什么会编译通过

因为门面不引用任何 native 方法，Java 侧完全自洽，`mvn compile` 不会报错。
问题只在运行期暴露，而唯一消费方
`utils-support-deeplearning-needle-starter` 的 `NeedleChatClient`
标注了 `@Spi("needle")`，会进入 SPI 注册表。

## 调用方应当如何处理

```java
if (!NeedleNative.isLoaded()) {
    log.warn("needle 未就绪: {}", NeedleNative.getLoadError().getMessage());
    return;   // 降级到其它推理后端，而不是继续调用
}
```

`NeedleChatClient#chatSync` 当前**没有**这道守卫，会直接抛
`IllegalStateException`。接入方需自行补降级，或在原生侧就绪前不要注册该 SPI。

## 提供动态库的方式

门面支持外部目录覆盖，无需把库打进 jar。按优先级：

1. 系统属性 `-Dchua.needle.native.dir=<目录>`
2. 环境变量 `CHUA_NEEDLE_NATIVE_DIR=<目录>`

目录下匹配 `*needle*` 且以 `.dll` / `.so` / `.dylib` 结尾的文件会被加载。
两者都找不到时，`isLoaded()` 返回 `false`，`getLoadError()` 给出含排查指引的原因。

## 补齐原生侧时需要做的事

1. 提供 `chua_native_needle` 的可执行产物（四平台），置于
   `src/main/resources/native/{platform}/` 或使用上述外部目录覆盖；
2. 在 `NeedleNative` 中用 `Linker` / `SymbolLookup` 绑定导出符号，
   替换 `requireBinding()` 中的 `UnsupportedOperationException`；
3. 去掉本文件顶部的状态说明，并把 `NeedleChatClient` 的降级逻辑接上。
