//! **BZ-c**：事件 predicate 的稀疏推进日志（`SeekKind::EventReplay` / L2）。
//!
//! 只记「predicate 为真的帧号」（升序）。`State(frame) = f^{#matched ≤ frame}(init)`
//! ⇒ 帧间是 identity，seek 到任意过去帧只需二分计数（`O(log 事件数)`）；
//! 代价不随总帧数增长 —— 这就是"只重放事件帧"的落点。
//!
//! 日志是**运行期状态**（Resource），不进产物；下标 = 全局 `Step::State` 槽位
//! （按 `BindingTable` 的绑定顺序 + 绑定内计划顺序展平）。

/// 每个事件 predicate `State` 的匹配帧号（升序）。
#[derive(Clone, Debug, Default)]
pub struct StateEventLog {
    entries: Vec<Vec<i64>>,
}

impl StateEventLog {
    /// 确保至少有 `len` 个槽位。
    pub fn slots(&mut self, len: usize) {
        if self.entries.len() < len {
            self.entries.resize(len, Vec::new());
        }
    }

    /// 记录一次推进（`frame ≥ 1`；同一帧重复调用不会重复记录）。
    pub fn push(&mut self, slot: usize, frame: i64) {
        self.slots(slot + 1);
        if self.entries[slot].last() != Some(&frame) {
            self.entries[slot].push(frame);
        }
    }

    /// 某个绑定的计划日志切片（下标与 `Binding::state_plans` 对齐）。
    pub fn slice(&self, start: usize, len: usize) -> &[Vec<i64>] {
        self.entries.get(start..start + len).unwrap_or(&[])
    }

    /// 全部槽位（诊断用）。
    pub fn entries(&self) -> &[Vec<i64>] {
        &self.entries
    }
}
