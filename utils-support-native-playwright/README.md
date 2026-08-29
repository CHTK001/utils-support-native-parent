# utils-support-native-playwright

Native Rust/C 预编译二进制库。

## 功能场景

见调用方项目说明。

## 构建说明

Windows DLL 已预编译。Linux .so 需在 Docker 环境编译:
\\\ash
docker run --rm -v \D:\ch\project:/src rust:latest bash -c "cd /src && cargo build --release --target x86_64-unknown-linux-gnu"
\\\

## 被谁使用

调用方见各自 pom.xml 依赖配置。
