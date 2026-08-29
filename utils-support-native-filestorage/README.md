# utils-support-native-filestorage

高性能文件存储处理器 Rust native 库，提供文件处理能力（缩放、水印、裁剪、旋转、滤镜等）。

## 功能场景

- 文件上传处理（缩放/水印/裁剪）
- 图片过滤器链
- 文件排除规则

## 平台支持

| 平台 | 状态 | 备注 |
|------|------|------|
| Windows x86_64 | 需编译 | 运行 ./build.sh windows x86_64 release |
| Linux x86_64 | 需编译 | 运行 ./build.sh linux x86_64 release |

## 构建

cd src/main/rust
./build.sh auto auto release

Linux: docker run --rm -v D:\ch\project:/src rust:latest bash -c "cd /src && ./build.sh linux x86_64 release"

## JNI 接口

- native_init() - 初始化
- native_get_version() - 获取版本
- native_get_capabilities() - 获取能力列表
- native_parse_params(json) - 解析参数 JSON
- native_get_filter_capabilities() - 获取过滤器能力
- native_get_filter_chain_json() - 获取过滤器链
- native_is_excluded(path, ext) - 检查是否被排除

## 被谁使用

调用方: utils-support-filesystem-parent/utils-support-filestorage-starter
