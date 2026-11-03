//! **CG-g**：Headless 出口 —— 零窗口 / 零 GPU / 零 egui 也能跑一帧结构化 UI。
//!
//! ```text
//! 注入 tagged JSON 事件流 → 纯计算（场景 System）→ 一帧 Frame（JSON）→ 断言
//! ```
//!
//! # 它为什么能零 GPU
//!
//! 装配被拆成两半（`zela-playback`）：
//! - [`zela_playback::assemble_scheduler`]：资源 + 系统图，**不认识渲染上下文**；
//! - [`zela_playback::assemble_runtime`]：在调度器外面加 `RenderContext`。
//!
//! 本模块只用前半段 ⇒ 不开窗口、不建 wgpu 设备、不链接 egui。
//! 同一条装配路径也被 GUI 用（`crate::build_scene_assembly`），所以这里跑出的一帧
//! 就是 GUI 那一帧的**同一份计算**，不是另一条"测试专用"路径。
//!
//! # Agent 读什么
//!
//! [`HeadlessScene::frame_json`] 返回 `ze_interact::Frame` 的 JSON（几何 + 语义 +
//! 文本三条平行结构）。断言读的是 `Frame::value_of(role, name)`，**不是像素 hash、
//! 不是 Rust 内部状态** —— 换实现（画法 / 后端）不会让断言失真。

use std::sync::Arc;

use ze_interact::Frame;
use zela_scene_ecs::domain::{InputEvents, SceneEvent};
use zela_scene_ecs::hud::HudState;
use zela_scene_ecs::{DriverCommand, DriverPod};

use crate::{Result, SceneChoice};

/// 一次 headless 装配：只有一个驱动器和一份时长（没有渲染上下文）。
pub struct HeadlessScene {
    driver: Arc<DriverPod>,
    duration: f64,
}

impl HeadlessScene {
    /// 时钟树给出的默认时长（秒）。
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// **注入 tagged JSON 事件流**（与跨进程输入流同一格式）。
    ///
    /// 返回注入的事件数；解析失败 ⇒ 明确错误（不静默丢事件）。
    pub fn inject_json(&self, json: &str) -> std::result::Result<usize, String> {
        let events: Vec<SceneEvent> =
            serde_json::from_str(json).map_err(|error| format!("事件流不是合法 JSON：{error}"))?;
        let count = events.len();
        self.driver.with_scheduler_mut(|scheduler| {
            scheduler
                .resources_mut()
                .insert(InputEvents::from_events(events));
        });
        Ok(count)
    }

    /// 清空本帧输入（下一帧不应重复消费同一批事件）。
    pub fn clear_events(&self) {
        self.driver.with_scheduler_mut(|scheduler| {
            scheduler
                .resources_mut()
                .insert(InputEvents::from_events(Vec::new()));
        });
    }

    /// 推进 `frames` 帧（固定 `dt`；与 GUI 的固定步长路径同一入口）。
    pub fn advance(&self, frames: u32, dt: f32) {
        self.driver.submit(DriverCommand::Batch { frames, dt });
    }

    /// 当前帧的 `Frame` JSON（`None` = 本场景没有自研面板）。
    ///
    /// 取的是**当前状态**的快照：布局不重解（`Hud::frame` 只做纯值重算）。
    pub fn frame_json(&self) -> Option<String> {
        self.driver.with_scheduler_mut(|scheduler| {
            let mut hud = scheduler.resources().get::<HudState>()?.hud()?.clone();
            let frame = hud.frame();
            serde_json::to_string(&frame).ok()
        })
    }

    /// 当前帧的 `Frame`（已解析的纯值；断言用）。
    pub fn frame(&self) -> Option<Frame> {
        let json = self.frame_json()?;
        serde_json::from_str(&json).ok()
    }

    /// 语义控件的**世界坐标**落点（Agent 把"点哪儿"换算成场景坐标的唯一入口）。
    ///
    /// `t` 是沿控件宽度的比例（`0` = 左端，`1` = 右端）。
    pub fn slider_point(&self, name: &str, t: f32) -> Option<(f32, f32)> {
        self.driver.with_scheduler(|scheduler| {
            let hud = scheduler.resources().get::<HudState>()?.hud()?;
            let id = hud.id_by_name(name)?;
            let rect = hud.world_rect(id)?;
            Some((
                rect.min.x + rect.size().x * t.clamp(0.0, 1.0),
                rect.center().y,
            ))
        })
    }
}

/// **装配一个 headless 场景**：与 GUI 相同的场景 / 系统图，但没有渲染上下文。
pub fn build(choice: SceneChoice) -> Result<HeadlessScene> {
    // GUI 默认叠加 demo3d（`build_scene_runtime` 同参）⇒ 两条路径的场景完全一致。
    let assembly = crate::build_scene_assembly(crate::SceneSource::Choice(choice), true)?;
    let assembled = zela_playback::assemble_scheduler(assembly);
    Ok(HeadlessScene {
        driver: Arc::new(DriverPod::new(assembled.scheduler)),
        duration: assembled.duration,
    })
}
