# utils-support-native-filestorage

高性能文件存储处理器 Rust native 库，提供文件处理能力（缩放、水印、裁剪、旋转、滤镜）。

## 功能场景

- 文件上传处理（缩放/水印/裁剪）
- 图片过滤器链
- 文件排除规则

> **注意**：HEIC/HEIF 预览转码能力已迁移至 `utils-support-image-starter` 的纯 Java 实现（`imageio-heif`），
> 本模块不再包含 HEIC 解码逻辑。

## 平台支持

| 平台 | 状态 | 备注 |
|------|------|------|
| Windows x86_64 | 需编译 | 运行 ./build.sh windows x86_64 release |
| Linux x86_64 | 需编译 | 运行 ./build.sh linux x86_64 release |

## 构建

```bash
cd src/main/rust
./build.sh auto auto release
```

Linux docker 构建示例：
```bash
docker run --rm -v D:\ch\project:/src rust:latest bash -c "cd /src && ./build.sh linux x86_64 release"
```

## 被谁使用

原生库本身不直接依赖 starter 模块；`utils-support-filestorage-starter` 现采用 JDK 实现，不再引入此 native 库。
