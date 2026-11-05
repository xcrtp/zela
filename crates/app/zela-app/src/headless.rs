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
use ze_scene_core::domain::{Injected, SceneEvent, SceneInputContext, WindowInput};
use zela_scene_ecs::domain::InputEvents;
use zela_scene_ecs::hud::HudState;
use zela_scene_ecs::{DriverCommand, DriverPod};

use crate::debug::{Diagnostics, TraceError, TraceReport};
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

    /// 驱动器句柄（**CH-f**：窗口层流回放等驱动器级原语需要它）。
    ///
    /// 与 GUI 的 `App::driver()` 同一个对象类型，因此回放走的是同一份运行时。
    pub fn driver(&self) -> Arc<DriverPod> {
        Arc::clone(&self.driver)
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

    /// **CH-a**：当前帧的窗口输入上下文（从视图声明推导，与 GUI 同一条）。
    ///
    /// 窗口坐标系 = `ViewRect`（headless 默认 800×600）；相机取景来自本帧视图声明。
    /// `None` = 场景没有任何可操作图层（纯空场景）⇒ 窗口输入无处可投。
    pub fn input_context(&self) -> Option<SceneInputContext> {
        crate::debug::input_context_of(&self.driver)
    }

    /// **CH-a**：注入**窗口输入**（逻辑像素 + 环境参数）—— 换算与窗口里真人
    /// 完全同一条 `collect_events`（视口判定 / 图层选择 / 相机 / HiDPI）。
    ///
    /// 返回派生出的场景事件数；环境缺失（没有可操作图层）⇒ 明确错误。
    pub fn inject_window_json(&self, json: &str) -> std::result::Result<usize, String> {
        let input: WindowInput = serde_json::from_str(json)
            .map_err(|error| format!("窗口输入不是合法 JSON：{error}"))?;
        self.inject_window(&input)
    }

    /// [`Self::inject_window_json`] 的强类型版本。
    pub fn inject_window(&self, input: &WindowInput) -> std::result::Result<usize, String> {
        let context = self
            .input_context()
            .ok_or_else(|| "场景没有可操作的图层（2D / 3D 都没有）".to_string())?;
        let events = ze_scene_core::domain::collect_events(input, &context);
        let count = events.len();
        // **CH-e**：窗口输入同时落资源 ⇒ 诊断快照如实报告尺寸 / DPI / 视口。
        crate::debug::store_window_input(&self.driver, input);
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

    /// 当前帧号（debug / RPC 的显式帧号来源）。
    pub fn frame_number(&self) -> u32 {
        self.driver.frame()
    }

    /// 当前派生态诊断（与 `trace` 里每帧的诊断同一口径）。
    pub fn diagnostics_snapshot(&self) -> Diagnostics {
        crate::debug::diagnostics_of(&self.driver)
    }

    /// **只投递，不驱动**：把一条带标记的事件放进本帧 `InputEvents`。
    ///
    /// 帧号必须等于当前帧（**一帧一个 active tag**）；不推进时间。
    pub fn inject_injected(&self, injected: &Injected) -> std::result::Result<usize, String> {
        crate::debug::inject_injected(&self.driver, injected)
    }

    /// **CH-b**：注入一个带 tag 的事件并返回**差分回执**。
    ///
    /// 与 GUI 的 debug RPC 共用 `crate::debug::trace_on`：
    /// - `max_frames` 是显式窗口上限（`0` 按 1 处理）；到达上限且最后一帧仍有变化
    ///   ⇒ `truncated = true`（如实说明，不假装收敛）；
    /// - 注入帧号必须等于当前帧（**一帧一个 active tag**）；
    /// - 派生事件来自 `collect_events`，`outcomes` 是状态机返回值的镜像，
    ///   `frame_diff` 覆盖所有 System 的副作用 ⇒ 零 System 传播 tag。
    pub fn trace(
        &self,
        injected: &Injected,
        max_frames: u32,
    ) -> std::result::Result<TraceReport, TraceError> {
        crate::debug::trace_on(&self.driver, injected, max_frames)
    }
    /// 推进 `frames` 帧（固定 `dt`；与 GUI 的固定步长路径同一入口）。
    pub fn advance(&self, frames: u32, dt: f32) {
        self.driver.submit(DriverCommand::Batch { frames, dt });
    }

    /// 当前帧的 `Frame` JSON（`None` = 本场景没有自研面板）。
    ///
    /// 取的是**当前状态**的快照：布局不重解（`Hud::frame` 只做纯值重算）。
    pub fn frame_json(&self) -> Option<String> {
        crate::debug::frame_json_of(&self.driver)
    }

    /// 当前帧的 `Frame`（已解析的纯值；断言用）。
    pub fn frame(&self) -> Option<Frame> {
        crate::debug::frame_of(&self.driver)
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
