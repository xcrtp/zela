//! 命令帧体（每条命令一个模块）。
//!
//! 阶段 1 先落两条：`render`（有帧）与 `list`（无帧）。
//! 其余命令在阶段 2 迁入，CLI 侧只留适配。

pub mod api;
pub mod batch;
pub mod check;
pub mod compile;
pub mod doctor;
pub mod fingerprint;
pub mod fonts;
pub mod frames;
pub mod inspect;
pub mod list;
pub mod render;
