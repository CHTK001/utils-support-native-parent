# utils-support-native-headless

无头浏览器 Rust 封装（预编译二进制）。

## 功能说明

提供 headless Chrome/Chromium 浏览器的 Rust native 封装，支持网页自动化、截图、数据抓取等功能。

## 平台支持

| 平台 | 状态 | 备注 |
|------|------|------|
| Windows x86_64 | ✅ 已预编译 | headless_rust.dll (10MB) |
| Linux x86_64 | ❌ 源码丢失 | 需外部提供 Rust 源码后重新编译 |

## 构建说明

Rust 源码已不可追溯，当前仅提供 Windows DLL。如需 Linux 支持，请提供原始源码后执行：
```bash
cd src/main/rust
cargo build --release --target x86_64-unknown-linux-gnu
```

## 被谁使用

调用方待对接（原 utils-support-headless-rust-starter 已删除）。
