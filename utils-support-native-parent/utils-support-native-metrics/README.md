# Metrics Native

系统指标 Rust native 库，通过 FlatBuffers 实现零拷贝跨语言传输。

## 构建

```bash
cd src/rust
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-unknown-linux-gnu
```

## 指标

| 指标 | 粒度 |
|------|------|
| CPU | 每核心 |
| 内存 | 每插槽 |
| 磁盘 | 每分区 + IO |
| 网络 | 每网卡 |
| 进程 | 前 N 条 |
| GPU | 每显卡 |
| 电池 | 每电池 |
| 系统负载 | Load 1/5/15 |

## 接口

```rust
pub fn start_sampler(interval_ms: u64);
pub fn get_latest_snapshot() -> Option<Vec<u8>>;
pub fn stop_sampler();
```