use thiserror::Error;

use crate::clock::ClockId;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TimeError {
    #[error("clock {0:?} not found")]
    ClockNotFound(ClockId),
    /// 时钟树成环：`chain` 是**完整环路径**（首尾相同，例如 `[A, B, A]`）。
    #[error("clock tree contains a cycle: {chain:?}")]
    CycleDetected { chain: Vec<ClockId> },
    #[error("reverse rate requires replay-kind seek: stateful animators have no O(1) reverse seek")]
    ReverseNotSeekable,
}
