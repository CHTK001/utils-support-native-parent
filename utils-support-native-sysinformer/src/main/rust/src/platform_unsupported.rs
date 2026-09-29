//! Windows 平台实现。
//!
//! # 契约
//! 本文件只需实现一个入口：`call(op, args) -> JSON`，返回值必须是
//! `crate::model::Envelope` 的序列化结果。**不支持的能力必须返回
//! `Envelope { ok: false, error: ... }`**（用 [`unsupported`] 生成），
//! 不要返回空集合——"不支持"与"支持但为空"调用方必须能区分。
//!
//! # 本文件的边界
//! 只放**平台专属**逻辑。三平台一致的字段请走 `crate::common`，
//! 不要复制一份。
//!
//! # 待实现的 op
//! 见仓库 README 的 op 一览表：`process.detail` / `process.threads` /
//! `process.env` / `process.handles` / `process.modules` / `process.credential` /
//! `process.mappings` / `process.stack` / `socket.list` / `kernel.modules` /
//! `service.list` / `action.exec` / `events.start` / `events.poll` / `events.stop` /
//! `disk.io` / `gpu.list` / `sensor.list` / `memory.modules`

use crate::common;
use crate::model::{unsupported, Envelope};
use crate::PLATFORM;

/// 平台入口：按 op 分发。
///
/// # 参数
/// * `op` - 操作名
/// * `args` - JSON 参数，无参时为空串
///
/// # 返回值
/// 序列化后的 `Envelope`
pub fn call(op: &str, args: &str) -> String {
    let _ = args;
    match op {
        // 跨平台基线已能覆盖的部分：先接上，平台专属字段待补
        "process.list" => {
            serde_json::to_string(&Envelope::ok(common::sysinfo_processes())).unwrap_or_default()
        }
        "process.tree" => {
            serde_json::to_string(&Envelope::ok(common::process_tree())).unwrap_or_default()
        }
        other => serde_json::to_string(&Envelope::<()>::err(unsupported(other, PLATFORM)))
            .unwrap_or_default(),
    }
}
