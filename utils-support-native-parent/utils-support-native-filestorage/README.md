# utils-support-native-filestorage

文件存储 Rust native 库，提供高性能的 URL 参数解析与图片滤镜处理。

---

## 功能

- **URL 参数解析**：通过 Rust 高性能解析 `?preview&size=200x200&format=webp` 等参数
- **图片滤镜**：Rust 实现的图片处理（resize、blur、grayscale 等），性能远超 Java
- **全局滤镜**：每次图片请求都会应用配置的滤镜链

---

## 依赖关系

```
utils-support-native-filestorage
├── (无内部 Java 依赖)
└── native binaries:
    ├── windows-x86_64/rust_filestorage_processor.dll
    └── linux-x86_64/librust_filestorage_processor.so
```

---

## Java 端调用

Java 端通过 `utils-support-filestorage-starter` 中的桥接类调用：

```java
// 自动降级：若 native 库未加载，则使用 JDK 实现
NativeFileStorageFileSetting setting = new NativeFileStorageFileSetting();
FileOperationSetting ops = setting.parse(request);
```

---

## 构建

Rust 源码位于 `src/main/rust`（需要单独构建）。运行时通过 SPI 自动发现 native 库，无需额外配置。