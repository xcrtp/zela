//! **CH-b**：`ActiveTags` + `TraceReport` —— 注入前后的**差分回执**。
//!
//! # 它不是全链路埋点
//!
//! 因果不靠传播 tag，靠**快照差分**：
//!
//! ```text
//! 注入前读一份 Frame / 状态快照
//!   → 带 tag 投入 ActiveTags（本帧唯一）
//!   → 推进一帧
//!   → 差分（Frame diff）+ collect_events 派生事件 + ControlOutcome 镜像
//!   → TraceReport
//! ```
//!
//! 每个 System 都不需要知道 tag —— 差分会覆盖**所有**副作用，所以不存在"漏报"，
//! 也没有隐式状态，确定性天然成立。
//!
//! # 两个口径（让它等价于因果的条件）
//!
//! 1. **一帧一个 active tag**：同帧注入多个标记事件，差分无法区分谁引起哪个变化
//!    ⇒ 要求分帧注入（或调用方自行合并）。与「帧号是一等通道」一致。
//! 2. **跨帧用显式窗口**：`until_frame` / `max_frames` 由调用方给（默认
//!    [`DEFAULT_TRACE_FRAMES`]），报告里写明是否**截断**。「自动判断何时算完」
//!    是隐式、不确定的。
//!
//! 诚实地说清限制：差分给的是「这一帧**所有**变化」，不是严格因果证明。
//! 上面两条口径正是让它等价于因果的条件。
//!
//! # 不进产物
//!
//! `TraceReport` 是纯值、可哈希、**不进 payload、不进输入流**（那条原则的第六次出现，
//! 见 CH-g 的 `principles.md`）。

use serde::{Deserialize, Serialize};

use ze_interact::Frame;
use ze_scene_core::domain::{
    FrameTag, InjectError, InjectSource, Injected, InjectedPayload, SceneInputContext, SceneEvent,
    TagId, WindowInput,
};
use zela_scene_ecs::contract::WorldView;
use zela_scene_ecs::domain::{InputEvents, UiConfig};
use zela_scene_ecs::hud::HudState;
use zela_scene_ecs::{DriverCommand, DriverPod, Hud};

/// 跨帧回执的默认上限（帧）。
pub const DEFAULT_TRACE_FRAMES: u32 = 8;

/// 当前活动的标记（**一帧一个**；`until_frame` 是显式窗口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveTags {
    pub tag: TagId,
    /// 显式窗口末端（含）：下一帧不再属于这个 tag。
    pub until_frame: u32,
}

impl ActiveTags {
    pub fn new(tag: TagId, until_frame: u32) -> Self {
        Self { tag, until_frame }
    }

    /// 该帧是否仍属于本标记。
    pub fn covers(&self, frame: u32) -> bool {
        frame <= self.until_frame
    }
}

/// 一次注入的完整回执。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceReport {
    pub tag: TagId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: InjectSource,
    pub injected_frame: u32,
    /// 窗口：[`ActiveTags`] 覆盖到的帧（显式上限内）。
    pub active: ActiveTags,
    pub frames: Vec<FrameTrace>,
    /// 到达上限且最后一帧仍有变化 ⇒ 报告被**截断**（如实说明）。
    pub truncated: bool,
}

/// 单帧回执。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameTrace {
    pub frame: u32,
    /// `collect_events` 派生出的场景事件（只出现在注入帧）。
    pub derived: Vec<SceneEventRecord>,
    /// 状态机产出的外部意图（镜像 `ControlOutcome`；kernel 不加 serde）。
    pub outcomes: Vec<OutcomeRecord>,
    pub frame_diff: FrameDiff,
    pub diagnostics: Diagnostics,
}

/// `SceneEvent` 的镜像（纯数据；与场景事件逐字段一致，便于断言 / 序列化）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneEventRecord {
    Hover { layer: u32, scene_pos: [f32; 2] },
    Click { layer: u32, scene_pos: [f32; 2] },
    Release { layer: u32, scene_pos: [f32; 2] },
    Camera { layer: u32, command: String },
    Other { debug: String },
}

impl From<&ze_scene_core::domain::SceneEvent> for SceneEventRecord {
    fn from(event: &ze_scene_core::domain::SceneEvent) -> Self {
        use ze_scene_core::domain::SceneEvent as E;
        match event {
            E::ViewportHover { layer, scene_pos } => Self::Hover {
                layer: layer.0,
                scene_pos: [scene_pos.x, scene_pos.y],
            },
            E::ViewportClick { layer, scene_pos } => Self::Click {
                layer: layer.0,
                scene_pos: [scene_pos.x, scene_pos.y],
            },
            E::ViewportRelease { layer, scene_pos } => Self::Release {
                layer: layer.0,
                scene_pos: [scene_pos.x, scene_pos.y],
            },
            E::Camera { layer, command } => Self::Camera {
                layer: layer.0,
                command: format!("{command:?}"),
            },
            other => Self::Other {
                debug: format!("{other:?}"),
            },
        }
    }
}

/// `ControlOutcome` 的镜像（debug 协议在 app 层，kernel 保持零 serde）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub control: u32,
    pub name: String,
    pub kind: OutcomeKind,
}

/// 意图类型（镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutcomeKind {
    ValueChanged { value: f64 },
    Activated,
}

/// Frame 的语义层差分（全覆盖：任何 System 的副作用最终都落在语义值上）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrameDiff {
    /// `role:name`（按 after 的声明顺序）。
    pub appeared: Vec<String>,
    /// `role:name`（按 before 的声明顺序）。
    pub disappeared: Vec<String>,
    pub changed: Vec<SemanticChange>,
}

impl FrameDiff {
    pub fn is_empty(&self) -> bool {
        self.appeared.is_empty() && self.disappeared.is_empty() && self.changed.is_empty()
    }
}

/// 单个语义锚点的值变化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticChange {
    pub role: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// 派生态诊断：命中谁 / 路由去向 / 为什么落空 / **装配环境**。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    /// capture > focus > hover 的当前投递目标（语义名）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed: Option<String>,
    /// 没有可观察帧时的说明（例如纯 3D 场景）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// **CH-e**：本帧的装配环境（窗口尺寸 / DPI / 视口 / 字体 face 清单）。
    #[serde(default)]
    pub environment: Environment,
}

/// **CH-e**：装配环境 —— 让同一个 `(x, y)` 在不同机器上的落点**可解释**。
///
/// 为什么必须显式：窗口坐标是逻辑像素，2× DPI 上同一个 `(x, y)` 落到不同控件；
/// 字体 face 不同则标签几何不同。二者都不进 payload（不是内容），但回执不写清楚，
/// 跨机器复现就只能靠猜。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Environment {
    /// 窗口尺寸（逻辑像素）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_size: Option<[f32; 2]>,
    /// HiDPI 缩放比（**不参与**换算，只作解释）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_factor: Option<f32>,
    /// 中央视口矩形 `[min.x, min.y, max.x, max.y]`（逻辑像素）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<[f32; 4]>,
    /// 字体 face 清单 / 版本（CD 的 `Baked` 角色：回放端不执行，不进
    /// `required_capabilities`；版本差异是可解释的差异）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fonts: Option<crate::fonts::FontEnvironment>,
}

/// **Frame 差分**：按 `(role, name)` 对齐，值不同 ⇒ `changed`。
pub fn diff_frames(before: Option<&Frame>, after: Option<&Frame>) -> FrameDiff {
    let (Some(before), Some(after)) = (before, after) else {
        return FrameDiff::default();
    };
    let mut diff = FrameDiff::default();
    for node in &after.semantics {
        match before.semantic(&node.role, &node.name) {
            None => diff.appeared.push(format!("{}:{}", node.role, node.name)),
            Some(old) if old.value != node.value => diff.changed.push(SemanticChange {
                role: node.role.clone(),
                name: node.name.clone(),
                before: old.value.clone(),
                after: node.value.clone(),
            }),
            Some(_) => {}
        }
    }
    for node in &before.semantics {
        if after.semantic(&node.role, &node.name).is_none() {
            diff.disappeared.push(format!("{}:{}", node.role, node.name));
        }
    }
    diff
}

/// 回执失败的原因（全部显式）。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TraceError {
    /// envelope 校验失败（External 未实现 / 来源与载荷不匹配）。
    #[error(transparent)]
    Inject(#[from] InjectError),
    /// 注入帧号与当前帧不一致（一帧一个 active tag）。
    #[error("注入帧号 {requested} 与当前帧 {current} 不一致（要求分帧注入）")]
    FrameMismatch { requested: u32, current: u32 },
    /// 注入 / 读取失败。
    #[error("{0}")]
    Input(String),
}

// ─── 驱动器级原语（GUI 与 headless 共用；零 egui 类型）────────────────────

/// 当前帧的结构化快照（`None` = 本场景没有自研面板）。
pub fn frame_of(driver: &DriverPod) -> Option<Frame> {
    let mut hud = hud_of(driver)?;
    Some(hud.frame())
}

/// 当前帧的 `Frame` JSON。
pub fn frame_json_of(driver: &DriverPod) -> Option<String> {
    serde_json::to_string(&frame_of(driver)?).ok()
}

/// 当前 HUD 快照（没有自研面板 ⇒ `None`）。
pub fn hud_of(driver: &DriverPod) -> Option<Hud> {
    driver.with_scheduler(|scheduler| scheduler.resources().get::<HudState>()?.hud().cloned())
}

/// 本帧状态机意图（镜像）。
pub fn outcomes_of(driver: &DriverPod) -> Vec<OutcomeRecord> {
    hud_of(driver)
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

/// 派生态诊断（当前投递目标；没有可观察帧时给出说明）。
///
/// **CH-e**：环境参数与 `routed` 一并回执 —— 同一个 `(x, y)` 在不同 DPI / 窗口尺寸
/// 上落到不同控件，不写明环境就无法判断"回执不同"是不是 bug。
pub fn diagnostics_of(driver: &DriverPod) -> Diagnostics {
    let environment = driver.with_scheduler(|scheduler| {
        let input = scheduler.resources().get::<WindowInput>();
        Environment {
            window_size: input.map(|input| [input.window_size.x, input.window_size.y]),
            scale_factor: input.map(|input| input.scale_factor),
            viewport: input.map(|input| {
                [
                    input.viewport.min.x,
                    input.viewport.min.y,
                    input.viewport.max.x,
                    input.viewport.max.y,
                ]
            }),
            fonts: crate::fonts::environment(),
        }
    });
    match hud_of(driver) {
        None => Diagnostics {
            routed: None,
            note: Some("本场景没有结构化帧（无自研面板），只有事件层回执".into()),
            environment,
        },
        Some(hud) => Diagnostics {
            routed: hud.routed_name().map(str::to_string),
            note: None,
            environment,
        },
    }
}

/// 从本帧视图声明取窗口输入上下文（与 GUI 的 `collect_events` 同一条）。
pub fn input_context_of(driver: &DriverPod) -> Option<SceneInputContext> {
    driver.with_scheduler(|scheduler| {
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

/// **CH-e**：把本帧窗口输入写进资源（诊断环境的唯一来源）。
///
/// GUI 在 `EguiSceneHost::begin_frame` 里写、headless 注入时写 —— 两条入口都落
/// 同一个资源，诊断快照因此不会出现"GUI 有环境、headless 没有"的分叉。
pub fn store_window_input(driver: &DriverPod, input: &WindowInput) {
    driver.with_scheduler_mut(|scheduler| {
        scheduler.resources_mut().insert(input.clone());
    });
}

/// **只投递，不驱动**：把一条带标记的事件放进本帧 `InputEvents`。
///
/// 窗口载荷同时写进 `WindowInput` 资源（**CH-e**）：诊断快照要如实反映"这一帧
/// 的窗口尺寸 / DPI / 视口"，否则注入的回执会说谎。
pub fn inject_injected(driver: &DriverPod, injected: &Injected) -> Result<usize, String> {
    injected.validate().map_err(|error| error.to_string())?;
    let current = driver.frame();
    if injected.frame != current {
        return Err(format!(
            "注入帧号 {} 与当前帧 {current} 不一致（要求分帧注入）",
            injected.frame
        ));
    }
    let events: Vec<SceneEvent> = match &injected.payload {
        InjectedPayload::Window(input) => {
            let context = input_context_of(driver)
                .ok_or_else(|| "场景没有可操作的图层（2D / 3D 都没有）".to_string())?;
            let events = ze_scene_core::domain::collect_events(input, &context);
            store_window_input(driver, input);
            events
        }
        InjectedPayload::Scene(event) => vec![event.clone()],
        InjectedPayload::External { .. } => unreachable!("validate 已拒绝 External"),
    };
    let count = events.len();
    driver.with_scheduler_mut(|scheduler| {
        scheduler
            .resources_mut()
            .insert(InputEvents::from_events(events));
        // **CH-f**：本帧输入来自注入 ⇒ 窗口层录制要记下这个 tag。
        scheduler
            .resources_mut()
            .insert(FrameTag(Some(injected.tag)));
    });
    Ok(count)
}

/// **注入 + 差分回执**（GUI 与 headless 共用同一实现）。
pub fn trace_on(
    driver: &DriverPod,
    injected: &Injected,
    max_frames: u32,
) -> Result<TraceReport, TraceError> {
    injected.validate()?;
    let current = driver.frame();
    if injected.frame != current {
        return Err(TraceError::FrameMismatch {
            requested: injected.frame,
            current,
        });
    }
    let max_frames = max_frames.max(1);

    let derived_events: Vec<SceneEvent> = match &injected.payload {
        InjectedPayload::Window(input) => {
            let context = input_context_of(driver)
                .ok_or_else(|| TraceError::Input("场景没有可操作的图层".into()))?;
            let events = ze_scene_core::domain::collect_events(input, &context);
            store_window_input(driver, input);
            events
        }
        InjectedPayload::Scene(event) => vec![event.clone()],
        InjectedPayload::External { .. } => unreachable!("validate 已拒绝 External"),
    };
    driver.with_scheduler_mut(|scheduler| {
        scheduler
            .resources_mut()
            .insert(InputEvents::from_events(derived_events.clone()));
    });

    let mut before = frame_of(driver);
    let mut frames = Vec::new();
    let mut quiet = false;
    for index in 0..max_frames {
        driver.submit(DriverCommand::Batch {
            frames: 1,
            dt: 1.0 / 60.0,
        });
        // 注入只发生在本帧：推进后立即清空，后续帧是"零输入"的纯传播。
        driver.with_scheduler_mut(|scheduler| {
            scheduler
                .resources_mut()
                .insert(InputEvents::from_events(Vec::new()));
        });
        let outcomes = outcomes_of(driver);
        let after = frame_of(driver);
        let frame_diff = diff_frames(before.as_ref(), after.as_ref());
        let diagnostics = diagnostics_of(driver);
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

/// **CH-f**：窗口层流回放 —— 每帧用**当前**场景上下文重算 `collect_events`。
///
/// 与真人完全相同的一条链：录到的是窗口坐标 + 环境参数，回放端不存"换算结果"，
/// 所以换机器 / 换 DPI 仍然落到同一个控件（`scale_factor` 不参与换算）。
///
/// 帧号是索引：`0..=最后一帧`。稀疏帧按"零输入"推进（不是跳过帧）。
pub fn replay_window_stream(
    driver: &DriverPod,
    stream: &zela_scene_bundle::WindowStream,
) -> Result<usize, TraceError> {
    let last = stream.frames.last().map(|entry| entry.frame).unwrap_or(0);
    let mut played = 0;
    for frame in 0..=last {
        let events: Vec<SceneEvent> = match stream.input_at(frame) {
            Some(input) => {
                let context = input_context_of(driver)
                    .ok_or_else(|| TraceError::Input("场景没有可操作的图层".into()))?;
                let events = ze_scene_core::domain::collect_events(input, &context);
                store_window_input(driver, input);
                played += 1;
                events
            }
            None => Vec::new(),
        };
        driver.with_scheduler_mut(|scheduler| {
            scheduler
                .resources_mut()
                .insert(InputEvents::from_events(events));
        });
        driver.submit(DriverCommand::Batch {
            frames: 1,
            dt: 1.0 / 60.0,
        });
    }
    Ok(played)
}

/// 把 `TraceError` 变成 RPC 错误文本（保持类型在 app 层）。
pub fn trace_error_text(error: &TraceError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_interact::{Point, Rect, SemanticNode};

    fn frame(entries: &[(&str, &str, Option<&str>)]) -> Frame {
        let mut frame = Frame::new();
        for (role, name, value) in entries {
            let mut node = SemanticNode::new(
                *role,
                *name,
                Rect::from_min_size(Point::new(0.0, 0.0), 10.0, 10.0),
            );
            if let Some(value) = value {
                node = node.with_value(*value);
            }
            frame.semantics.push(node);
        }
        frame
    }

    #[test]
    fn diff_reports_appeared_disappeared_and_changed_values() {
        let before = frame(&[("slider", "a", Some("0.5")), ("button", "gone", None)]);
        let after = frame(&[("slider", "a", Some("0.8")), ("button", "new", None)]);
        let diff = diff_frames(Some(&before), Some(&after));
        assert_eq!(diff.appeared, vec!["button:new".to_string()]);
        assert_eq!(diff.disappeared, vec!["button:gone".to_string()]);
        assert_eq!(
            diff.changed,
            vec![SemanticChange {
                role: "slider".into(),
                name: "a".into(),
                before: Some("0.5".into()),
                after: Some("0.8".into()),
            }]
        );
    }

    #[test]
    fn without_frames_the_diff_is_empty() {
        assert!(diff_frames(None, None).is_empty());
        assert!(diff_frames(Some(&Frame::new()), None).is_empty());
    }

    #[test]
    fn active_tag_covers_only_its_explicit_window() {
        let tag = ActiveTags::new(TagId(4), 11);
        assert!(tag.covers(11));
        assert!(!tag.covers(12));
    }
}
