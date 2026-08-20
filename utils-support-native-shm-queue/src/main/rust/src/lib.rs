//! shmqueue — Rust FFI 绑定
//!
//! 底层为 C 动态库 `libshmqueue`，提供 SPSC 无锁环形队列。
//! 通过 `libloading` 运行时加载，避免构建期链接依赖。

#![allow(non_camel_case_types)]

mod ffi;
mod queue;

pub use ffi::{Error, Mode};
pub use queue::{Message, ShmQueue};

/// 加载底层动态库（幂等，可重复调用）
pub fn init() -> Result<(), Error> {
    ffi::Lib::load().map(|_| ())
}