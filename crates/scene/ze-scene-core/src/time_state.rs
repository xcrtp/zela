// time_state.rs — 全局时间状态（Resource）
//
// 从 zela-egui::resources 迁移而来，因为它是核心时间状态，不属于 UI 层。

/// 全局时间 + 帧号（S3：帧号是**一等通道**）。
///
/// 由 `DirectorAdvanceSystem` 推进（Director 是唯一时间源）；
/// 其他 System 只读它来驱动动画 / 表达式求值。
#[derive(Debug, Default, Clone, Copy)]
pub struct TimeState {
    /// 全局时间（秒），由 `Director.global_t` 派生（不再逐帧累加）。
    pub elapsed: f32,
    /// 已推进的帧数（与 `Director.frame` 同源；暂停时冻结）。
    pub frame: u64,
}

impl TimeState {
    pub fn new() -> Self {
        Self {
            elapsed: 0.0,
            frame: 0,
        }
    }

    /// 只指定时间（帧号 0）—— 测试 / 手工驱动夹具用。
    pub fn at(elapsed: f32) -> Self {
        Self { elapsed, frame: 0 }
    }

    /// 时间 + 帧号。
    pub fn at_frame(elapsed: f32, frame: u64) -> Self {
        Self { elapsed, frame }
    }
}
