# Linux x86_64 构建即验收：cargo build --release + C-ABI 冒烟。
# 冒烟不过（dlopen / 符号解析 / RTF 转换 / 错误路径任一失败）则镜像构建失败。
#
# 用法（仓库根目录）：
#   docker build -t anydoc-ffi:linux-x86_64 \
#     -f utils-support-native-anydoc/src/main/rust/Dockerfile \
#     utils-support-native-anydoc/src/main/rust
FROM rust:slim
WORKDIR /src

# 先只拷清单，预热依赖缓存（清单不变时依赖下载层可复用）
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && printf '' > src/lib.rs && cargo build --release

# 真实源码编译
COPY src ./src
RUN cargo build --release

# C-ABI 冒烟：dlopen + 5 符号解析 + RTF 转换 + 错误路径
COPY smoke.c ./
RUN cc -O2 -o /tmp/smoke smoke.c -ldl && /tmp/smoke /src/target/release/libanydoc_ffi.so
