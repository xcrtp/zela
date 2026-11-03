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
use ze_scene_core::domain::{
    InjectError, Injected, InjectedPayload, SceneInputContext, SceneEvent, WindowInput,
};
use zela_scene_ecs::contract::WorldView;
use zela_scene_ecs::domain::{InputEvents, UiConfig};
use zela_scene_ecs::hud::{HudState};
use zela_scene_ecs::{DriverCommand, DriverPod, Hud};

use crate::debug::{
    ActiveTags, Diagnostics, FrameTrace, OutcomeKind, OutcomeRecord, SceneEventRecord,
    TraceError, TraceReport, diff_frames,
};
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

    /// **CH-a**：当前帧的窗口输入上下文（从视图声明推导，与 GUI 同一条）。
    ///
    /// 窗口坐标系 = `ViewRect`（headless 默认 800×600）；相机取景来自本帧视图声明。
    /// `None` = 场景没有任何可操作图层（纯空场景）⇒ 窗口输入无处可投。
    pub fn input_context(&self) -> Option<SceneInputContext> {
        self.driver.with_scheduler(|scheduler| {
            let config = scheduler
                .resources()
                .get::<UiConfig>()
                .cloned()
                .unwrap_or_default();
            let view = zela_render::frame::build_scene_view(
                &WorldView::new(scheduler.world()),
                scheduler.resources(),
                zela_render::FrameOptions::from_ui_config(&config),
            );
            let context = SceneInputContext::from_view(&view, config.viewport_camera);
            (context.layer2d.is_some() || context.layer3d.is_some()).then_some(context)
        })
    }

    /// **CH-a**：注入**窗口输入**（逻辑像素 + 环境参数）—— 换算与窗口里真人
    /// 完全同一条 `collect_events`（视口判定 / 图层选择 / 相机 / HiDPI）。
    ///
    /// 返回派生出的场景事件数；环境缺失（没有可操作图层）⇒ 明确错误。
    pub fn inject_window_json(&self, json: &str) -> std::result::Result<usize, String> {
        let input: WindowInput =
            serde_json::from_str(json).map_err(|error| format!("窗口输入不是合法 JSON：{error}"))?;
        self.inject_window(&input)
    }

    /// [`Self::inject_window_json`] 的强类型版本。
    pub fn inject_window(&self, input: &WindowInput) -> std::result::Result<usize, String> {
        let context = self
            .input_context()
            .ok_or_else(|| "场景没有可操作的图层（2D / 3D 都没有）".to_string())?;
        let events = ze_scene_core::domain::collect_events(input, &context);
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

    /// **CH-b**：注入一个带 tag 的事件并返回**差分回执**。
    ///
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
        injected.validate()?;
        let current = self.driver.frame();
        if injected.frame != current {
            return Err(TraceError::FrameMismatch {
                requested: injected.frame,
                current,
            });
        }
        let max_frames = max_frames.max(1);

        // 1) 派生事件：窗口输入走纯函数换算；场景事件原样投递。
        let derived_events: Vec<SceneEvent> = match &injected.payload {
            InjectedPayload::Window(input) => {
                let context = self.input_context().ok_or_else(|| {
                    TraceError::Input("场景没有可操作的图层（2D / 3D 都没有）".into())
                })?;
                ze_scene_core::domain::collect_events(input, &context)
            }
            InjectedPayload::Scene(event) => vec![event.clone()],
            InjectedPayload::External { .. } => unreachable!("validate 已拒绝 External"),
        };
        self.driver.with_scheduler_mut(|scheduler| {
            scheduler
                .resources_mut()
                .insert(InputEvents::from_events(derived_events.clone()));
        });

        // 2) 逐帧推进 + 差分（不因请求多跑一帧：注入只发生在本帧）。
        let mut before = self.frame();
        let mut frames = Vec::new();
        let mut quiet = false;
        for index in 0..max_frames {
            self.advance(1, 1.0 / 60.0);
            self.clear_events();
            let outcomes = self.hud_outcomes();
            let after = self.frame();
            let frame_diff = diff_frames(before.as_ref(), after.as_ref());
            let diagnostics = self.diagnostics();
            let derived = if index == 0 {
                derived_events.iter().map(SceneEventRecord::from).collect()
            } else {
                Vec::new()
            };
            quiet = derived.is_empty() && outcomes.is_empty() && frame_diff.is_empty();
            frames.push(FrameTrace {
                frame: current + index + 1,
                derived,
                outcomes,
                frame_diff,
                diagnostics,
            });
            before = after;
            if quiet {
                break;
            }
        }

        Ok(TraceReport {
            tag: injected.tag,
            label: injected.label.clone(),
            source: injected.source,
            injected_frame: current,
            active: ActiveTags::new(injected.tag, current + max_frames),
            frames,
            truncated: !quiet,
        })
    }

    /// 当前 HUD 快照（没有自研面板 ⇒ `None`）。
    fn hud_snapshot(&self) -> Option<Hud> {
        self.driver
            .with_scheduler(|scheduler| scheduler.resources().get::<HudState>()?.hud().cloned())
    }

    /// 本帧状态机意图（镜像）。
    fn hud_outcomes(&self) -> Vec<OutcomeRecord> {
        self.hud_snapshot()
            .map(|hud| {
                hud.outcomes()
                    .iter()
                    .map(|outcome| OutcomeRecord {
                        control: outcome.control.0,
                        name: outcome.name.clone(),
                        kind: match outcome.kind {
                            zela_scene_ecs::HudOutcomeKind::ValueChanged(value) => {
                                OutcomeKind::ValueChanged { value }
                            }
                            zela_scene_ecs::HudOutcomeKind::Activated => OutcomeKind::Activated,
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 派生态诊断：当前投递目标；没有可观察帧时给出说明。
    fn diagnostics(&self) -> Diagnostics {
        let Some(hud) = self.hud_snapshot() else {
            return Diagnostics {
                routed: None,
                note: Some("本场景没有结构化帧（无自研面板），只有事件层回执".into()),
            };
        };
        Diagnostics {
            routed: hud.routed_name().map(str::to_string),
            note: None,
        }
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
