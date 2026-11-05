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
//! # 三条反面约束（比 API 列表重要）
//!
//! | 不要 | 为什么 |
//! |---|---|
//! | **做成全链路埋点 / 动态追踪** | 每个 System 传播 tag 会改遍全仓、必漏，还引入隐式状态，破坏确定性 |
//! | **让 tag 进 `SceneEvent`** | 标记是调试派生态、不属于作品输入；窗口事件本来也不是 `SceneEvent`，所以 tag 放在 `Injected` 外层 |
//! | **靠 System 手工上报响应** | 会漏、会变成第二套；差分是**全覆盖**的，不需要任何人配合 |
//!
//! 形状像分布式追踪，代价差一个量级：**纯函数 + 快照模型里，因果可以算出来，不需要被传播。**
//!
//! # 不进产物
//!
//! `TraceReport` 是纯值、可哈希、**不进 payload、不进输入流**（那条原则的第六次出现，
//! 见 `docs/principles.md` 的「⑥ 同一条约束反复出现」）。

use serde::{Deserialize, Serialize};

use ze_interact::Frame;
use ze_scene_core::domain::{
    FrameTag, InjectError, InjectSource, Injected, InjectedPayload, SceneEvent, SceneInputContext,
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
///
/// **CJ-05（B2）**：四类输入形状（`Pointer` / `Key` / `Text` / `Command`）与控件事件
/// 都有结构化变体；不再用 `format!("{other:?}")` 兜底（`Debug` 输出不保证稳定序列化）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneEventRecord {
    Hover {
        layer: u32,
        scene_pos: [f32; 2],
    },
    Click {
        layer: u32,
        scene_pos: [f32; 2],
    },
    Release {
        layer: u32,
        scene_pos: [f32; 2],
    },
    Camera {
        layer: u32,
        command: String,
    },
    Toggle {
        id: u32,
        value: bool,
    },
    Slider {
        id: u32,
        value: f32,
    },
    Button {
        id: u32,
    },
    PointerMoved {
        scene_pos: [f32; 2],
    },
    PointerPressed {
        scene_pos: [f32; 2],
        button: String,
    },
    PointerReleased {
        scene_pos: [f32; 2],
        button: String,
    },
    PointerWheel {
        scene_pos: [f32; 2],
        delta: [f32; 2],
    },
    KeyDown {
        key: String,
        modifiers: ModifierRecord,
    },
    KeyUp {
        key: String,
        modifiers: ModifierRecord,
    },
    TextCommit {
        text: String,
    },
    TextCompose {
        text: String,
        composing: bool,
    },
    TextAnchor {
        target: String,
        edge: String,
    },
    CommandInvoke {
        name: String,
    },
    CommandSetValue {
        name: String,
        value: f64,
    },
    /// **CK-d**：结构变化（稳定 id + 显式 op；不落成 Debug 字符串）。
    StructureDelta {
        ops: Vec<StructureOpRecord>,
    },
}

/// `StructureOp` 的镜像。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StructureOpRecord {
    Mount { id: u32, parent: Option<u32> },
    Unmount { id: u32 },
    Reparent { id: u32, parent: Option<u32> },
}

/// 修饰键的镜像。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModifierRecord {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl From<&ze_scene_core::domain::event::KeyModifiers> for ModifierRecord {
    fn from(modifiers: &ze_scene_core::domain::event::KeyModifiers) -> Self {
        Self {
            shift: modifiers.shift,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            meta: modifiers.meta,
        }
    }
}

fn pointer_button_name(button: ze_scene_core::domain::event::PointerButton) -> &'static str {
    use ze_scene_core::domain::event::PointerButton;
    match button {
        PointerButton::Primary => "primary",
        PointerButton::Secondary => "secondary",
        PointerButton::Middle => "middle",
    }
}

impl From<&ze_scene_core::domain::SceneEvent> for SceneEventRecord {
    fn from(event: &ze_scene_core::domain::SceneEvent) -> Self {
        use ze_scene_core::domain::event::{
            CommandInput, KeyInput, PointerInput, SceneEvent as E, TextAnchorEdge, TextInput,
        };
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
            E::Toggle { id, value } => Self::Toggle {
                id: id.0,
                value: *value,
            },
            E::Slider { id, value } => Self::Slider {
                id: id.0,
                value: *value,
            },
            E::Button { id } => Self::Button { id: id.0 },
            E::Pointer { input } => match input {
                PointerInput::Moved { scene_pos } => Self::PointerMoved {
                    scene_pos: [scene_pos.x, scene_pos.y],
                },
                PointerInput::Pressed { scene_pos, button } => Self::PointerPressed {
                    scene_pos: [scene_pos.x, scene_pos.y],
                    button: pointer_button_name(*button).to_string(),
                },
                PointerInput::Released { scene_pos, button } => Self::PointerReleased {
                    scene_pos: [scene_pos.x, scene_pos.y],
                    button: pointer_button_name(*button).to_string(),
                },
                PointerInput::Wheel { scene_pos, delta } => Self::PointerWheel {
                    scene_pos: [scene_pos.x, scene_pos.y],
                    delta: [delta.x, delta.y],
                },
            },
            E::Key { input } => match input {
                KeyInput::Down { key, modifiers } => Self::KeyDown {
                    key: key.clone(),
                    modifiers: modifiers.into(),
                },
                KeyInput::Up { key, modifiers } => Self::KeyUp {
                    key: key.clone(),
                    modifiers: modifiers.into(),
                },
            },
            E::Text { input } => match input {
                TextInput::Commit { text } => Self::TextCommit { text: text.clone() },
                TextInput::Compose { text, composing } => Self::TextCompose {
                    text: text.clone(),
                    composing: *composing,
                },
                TextInput::Anchor { anchor } => Self::TextAnchor {
                    target: anchor.target.clone(),
                    edge: match anchor.edge {
                        TextAnchorEdge::Start => "start".to_string(),
                        TextAnchorEdge::End => "end".to_string(),
                    },
                },
            },
            E::Command { input } => match input {
                CommandInput::Invoke { name } => Self::CommandInvoke { name: name.clone() },
                CommandInput::SetValue { name, value } => Self::CommandSetValue {
                    name: name.clone(),
                    value: *value,
                },
            },
            E::StructureDelta { ops } => Self::StructureDelta {
                ops: ops
                    .iter()
                    .map(|op| match op {
                        ze_scene_core::domain::StructureOp::Mount { id, parent } => {
                            StructureOpRecord::Mount {
                                id: id.0,
                                parent: parent.map(|parent| parent.0),
                            }
                        }
                        ze_scene_core::domain::StructureOp::Unmount { id } => {
                            StructureOpRecord::Unmount { id: id.0 }
                        }
                        ze_scene_core::domain::StructureOp::Reparent { id, parent } => {
                            StructureOpRecord::Reparent {
                                id: id.0,
                                parent: parent.map(|parent| parent.0),
                            }
                        }
                    })
                    .collect(),
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

/// Frame 的差分（覆盖**三条平行结构**：语义值 / 几何布局 / 文本）。
///
/// **CJ-05（B1）**：`Frame` 是 `geometry` / `semantics` / `text` 三条平行结构；只比
/// 语义值会漏掉布局重排、标签文本、命中区域的变化。这里对三条结构分别差分。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrameDiff {
    /// `role:name`（按 after 的声明顺序）。
    pub appeared: Vec<String>,
    /// `role:name`（按 before 的声明顺序）。
    pub disappeared: Vec<String>,
    /// 语义值变化。
    pub changed: Vec<SemanticChange>,
    /// `focusable` 等非值标记变化。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags_changed: Vec<SemanticChange>,
    /// 语义范围（命中区域）变化。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bounds_changed: Vec<BoundsChange>,
    /// 布局中新增的控件 id。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geometry_appeared: Vec<u32>,
    /// 布局中消失的控件 id。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geometry_disappeared: Vec<u32>,
    /// 布局落点 / 尺寸变化。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geometry_changed: Vec<GeometryChange>,
    /// 文本变化（按 run 下标对齐）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_changed: Vec<TextChange>,
    /// **CK-d**：父子索引变化（拖进 / 拖出容器、动态增删）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_changed: Vec<ParentChange>,
}

impl FrameDiff {
    pub fn is_empty(&self) -> bool {
        self.appeared.is_empty()
            && self.disappeared.is_empty()
            && self.changed.is_empty()
            && self.flags_changed.is_empty()
            && self.bounds_changed.is_empty()
            && self.geometry_appeared.is_empty()
            && self.geometry_disappeared.is_empty()
            && self.geometry_changed.is_empty()
            && self.text_changed.is_empty()
            && self.parent_changed.is_empty()
    }
}

/// 语义锚点的值变化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticChange {
    pub role: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// 语义范围变化（命中区域）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundsChange {
    pub role: String,
    pub name: String,
    pub before: [f64; 4],
    pub after: [f64; 4],
}

/// 布局落点 / 尺寸变化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryChange {
    pub id: u32,
    pub before: [f64; 4],
    pub after: [f64; 4],
}

/// 文本变化（`before` / `after` 是行高 + 逐字符盒的规范摘要）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextChange {
    pub run: usize,
    pub before: String,
    pub after: String,
}

/// **CK-d**：父子索引变化（声明期下标，不是运行期句柄）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentChange {
    /// 控件声明期下标（与 `Frame::geometry` 同一口径）。
    pub id: u32,
    pub before: Option<u32>,
    pub after: Option<u32>,
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
    /// **CK-e（#17 方案 B）**：控件回写所在相位。
    ///
    /// 静态冲突检查拒绝把 `HudSystem` 前移到 `Input` / `LogicUpdate`
    /// （与 `TimeControlSystem` / `ParamPanelSystem` 争写同一批资源），
    /// 因此承认跨帧：这里如实写相位与延迟帧数，**不假装同帧**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writeback_phase: Option<String>,
    /// 控件回写的跨帧延迟（帧）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writeback_delay_frames: Option<u32>,
    /// **CI-d 口子⑥**：内容重建后端 / face 版本（可扩展；不进
    /// `required_capabilities` —— 版本差异仍是"可解释的差异"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentDiagnostics>,
}

/// **CI-d 口子⑥**：内容路径的诊断（只报事实，不参与可还原性校验）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ContentDiagnostics {
    /// 这次重建用的是哪个后端（例如 `typst` / `direct-text`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// 实际使用的 face 版本（字体数据指纹）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_version: Option<u64>,
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
    /// **CK-g**：主题 id（控件只存 id；颜色由宿主解析）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

/// **Frame 差分**：三条平行结构分别对齐；值 / 范围 / 布局 / 文本任一变化都进回执。
pub fn diff_frames(before: Option<&Frame>, after: Option<&Frame>) -> FrameDiff {
    let (Some(before), Some(after)) = (before, after) else {
        return FrameDiff::default();
    };
    let mut diff = FrameDiff::default();
    for node in &after.semantics {
        match before.semantic(&node.role, &node.name) {
            None => diff.appeared.push(format!("{}:{}", node.role, node.name)),
            Some(old) => {
                if old.value != node.value {
                    diff.changed.push(SemanticChange {
                        role: node.role.clone(),
                        name: node.name.clone(),
                        before: old.value.clone(),
                        after: node.value.clone(),
                    });
                }
                if old.bounds != node.bounds {
                    diff.bounds_changed.push(BoundsChange {
                        role: node.role.clone(),
                        name: node.name.clone(),
                        before: rect_arr(old.bounds),
                        after: rect_arr(node.bounds),
                    });
                }
                if old.focusable != node.focusable {
                    diff.flags_changed.push(SemanticChange {
                        role: node.role.clone(),
                        name: node.name.clone(),
                        before: Some(format!("focusable={}", old.focusable)),
                        after: Some(format!("focusable={}", node.focusable)),
                    });
                }
            }
        }
    }
    for node in &before.semantics {
        if after.semantic(&node.role, &node.name).is_none() {
            diff.disappeared
                .push(format!("{}:{}", node.role, node.name));
        }
    }

    // 几何：按声明期 id 对齐（`PlacedRect::id` 是布局下标，不是运行期句柄）。
    for placed in &after.geometry {
        match before.geometry.iter().find(|old| old.id == placed.id) {
            None => diff.geometry_appeared.push(placed.id.0),
            Some(old) if old.rect != placed.rect => diff.geometry_changed.push(GeometryChange {
                id: placed.id.0,
                before: rect_arr(old.rect),
                after: rect_arr(placed.rect),
            }),
            Some(_) => {}
        }
    }
    for placed in &before.geometry {
        if !after.geometry.iter().any(|new| new.id == placed.id) {
            diff.geometry_disappeared.push(placed.id.0);
        }
    }

    // 文本：按 run 下标对齐；行高或任一字符盒不同 ⇒ 记一条。
    for (index, run) in after.text.iter().enumerate() {
        let after_summary = text_run_summary(run);
        let before_summary = before.text.get(index).map(text_run_summary);
        if before_summary.as_deref() != Some(after_summary.as_str()) {
            diff.text_changed.push(TextChange {
                run: index,
                before: before_summary.unwrap_or_default(),
                after: after_summary,
            });
        }
    }
    for index in after.text.len()..before.text.len() {
        diff.text_changed.push(TextChange {
            run: index,
            before: text_run_summary(&before.text[index]),
            after: String::new(),
        });
    }

    // **CK-d**：父子索引（与几何同下标）。结构变化必须能差分出来，
    // 否则动态 UI 的归因链只看到「Frame 变了」、看不到「树变了」。
    let parent_len = after.parent.len().max(before.parent.len());
    for index in 0..parent_len {
        let before_parent = before.parent.get(index).copied().flatten();
        let after_parent = after.parent.get(index).copied().flatten();
        if before_parent != after_parent {
            let id = after
                .geometry
                .get(index)
                .or_else(|| before.geometry.get(index))
                .map(|placed| placed.id.0)
                .unwrap_or(index as u32);
            diff.parent_changed.push(ParentChange {
                id,
                before: before_parent.map(|parent| parent.0),
                after: after_parent.map(|parent| parent.0),
            });
        }
    }
    diff
}

fn rect_arr(rect: ze_interact::Rect) -> [f64; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
}

/// 文本 run 的规范摘要：行高 + 逐字符（字符 / 四元矩形），用稳定格式拼接。
fn text_run_summary(run: &ze_interact::TextRun) -> String {
    let mut out = format!("h={:.3};", run.line_height);
    for cell in &run.chars {
        let rect = rect_arr(cell.rect);
        out.push_str(&format!(
            "{}@{:.3},{:.3},{:.3},{:.3};",
            cell.ch, rect[0], rect[1], rect[2], rect[3]
        ));
    }
    out
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
    let mut environment = driver.with_scheduler(|scheduler| {
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
            theme: None,
        }
    });
    let hud = hud_of(driver);
    if let Some(hud) = &hud {
        environment.theme = Some(hud.theme().to_string());
    }
    match hud {
        None => Diagnostics {
            routed: None,
            note: Some("本场景没有结构化帧（无自研面板），只有事件层回执".into()),
            environment,
            writeback_phase: None,
            writeback_delay_frames: None,
            content: None,
        },
        Some(hud) => Diagnostics {
            routed: hud.routed_name().map(str::to_string),
            note: hud.error_note(),
            environment,
            // 方案 B：回写发生在 FrameEnd，下一帧才被渲染消费。
            writeback_phase: Some("FrameEnd".to_string()),
            writeback_delay_frames: Some(1),
            content: None,
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
        InjectedPayload::External { .. } => {
            unreachable!("validate 已拒绝 External")
        }
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
        InjectedPayload::External { .. } => {
            unreachable!("validate 已拒绝 External")
        }
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

/// **CI-a**：窗口层流 → `TraceReport`（与离线 / RPC 同一个 [`trace_on`]）。
///
/// 窗口流里带 `tag` 的那一帧就是「注入帧」：
/// 1. 先按帧号重放它之前的窗口输入（同一个 `collect_events`，不存换算结果）；
/// 2. 到注入帧后调用 [`trace_on`] —— 与 GUI / RPC 完全同一条实现。
///
/// 只有一条流、一个 tag：多 tag 的流仍应分帧录制（一帧一个 active tag 的口径）。
pub fn trace_window_stream(
    driver: &DriverPod,
    stream: &zela_scene_bundle::WindowStream,
    max_frames: u32,
) -> Result<TraceReport, TraceError> {
    let Some(entry) = stream.frames.iter().find(|entry| entry.tag.is_some()) else {
        return Err(TraceError::Input(
            "窗口流没有带标记的帧（tag = None）".into(),
        ));
    };
    let tag = entry.tag.expect("已判 Some");

    for frame in 0..entry.frame {
        let events: Vec<SceneEvent> = match stream.input_at(frame) {
            Some(input) => {
                let context = input_context_of(driver)
                    .ok_or_else(|| TraceError::Input("场景没有可操作的图层".into()))?;
                let events = ze_scene_core::domain::collect_events(input, &context);
                store_window_input(driver, input);
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

    let current = driver.frame();
    if current != entry.frame {
        return Err(TraceError::FrameMismatch {
            requested: entry.frame,
            current,
        });
    }
    let injected = Injected::window(tag, entry.frame, entry.input.clone());
    trace_on(driver, &injected, max_frames)
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

    /// **CJ-05（B1）**：布局重排、标签文本变化、命中区域变化都必须进 `FrameDiff`。
    #[test]
    fn diff_covers_geometry_text_and_bounds() {
        use ze_interact::{LayoutId, PlacedRect, TextRun};

        let semantic = |bounds: Rect| {
            let mut frame = Frame::new();
            frame
                .semantics
                .push(SemanticNode::new("label", "status", bounds));
            frame
        };
        let before_bounds = Rect::from_min_size(Point::new(0.0, 0.0), 10.0, 10.0);
        let after_bounds = Rect::from_min_size(Point::new(0.0, 5.0), 10.0, 10.0);
        let before = semantic(before_bounds);
        let after = semantic(after_bounds);
        let diff = diff_frames(Some(&before), Some(&after));
        assert_eq!(
            diff.bounds_changed,
            vec![BoundsChange {
                role: "label".into(),
                name: "status".into(),
                before: [0.0, 0.0, 10.0, 10.0],
                after: [0.0, 5.0, 10.0, 15.0],
            }]
        );

        // 几何：同一个 layout id 落点变了 ⇒ geometry_changed。
        let mut before = Frame::new();
        before.geometry.push(PlacedRect {
            id: LayoutId(3),
            rect: Rect::from_min_size(Point::new(0.0, 0.0), 10.0, 10.0),
        });
        before.text.push(TextRun::new(12.0));
        let mut after = before.clone();
        after.geometry[0].rect = Rect::from_min_size(Point::new(40.0, 0.0), 10.0, 10.0);
        // 文本：标签内容变化 ⇒ text_changed。
        after.text[0] = TextRun::new(12.0);
        after.text[0].push(0, 'O', Point::new(4.0, 4.0), 6.0);
        after.text[0].push(1, 'K', Point::new(10.0, 4.0), 6.0);

        let diff = diff_frames(Some(&before), Some(&after));
        assert_eq!(diff.geometry_changed.len(), 1);
        assert_eq!(diff.geometry_changed[0].id, 3);
        assert_eq!(diff.geometry_changed[0].before, [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(diff.geometry_changed[0].after, [40.0, 0.0, 50.0, 10.0]);
        assert_eq!(diff.text_changed.len(), 1);
        assert!(diff.text_changed[0].after.contains("O@"));
        assert!(diff.text_changed[0].after.contains("K@"));
        assert!(!diff.is_empty(), "三条平行结构的任一变化都不得判成空差分");
    }

    /// **CK-d**：结构变化（父子索引）必须进回执，否则动态 UI 的归因链缺一半。
    #[test]
    fn diff_reports_parent_index_changes() {
        use ze_interact::{LayoutId, PlacedRect};

        let mut before = Frame::new();
        before.geometry.push(PlacedRect {
            id: LayoutId(0),
            rect: Rect::from_min_size(Point::new(0.0, 0.0), 10.0, 10.0),
        });
        before.geometry.push(PlacedRect {
            id: LayoutId(1),
            rect: Rect::from_min_size(Point::new(0.0, 20.0), 10.0, 10.0),
        });
        before.parent = vec![None, Some(LayoutId(0))];

        let mut after = before.clone();
        after.parent = vec![None, None];
        let diff = diff_frames(Some(&before), Some(&after));
        assert_eq!(
            diff.parent_changed,
            vec![ParentChange {
                id: 1,
                before: Some(0),
                after: None,
            }]
        );
        assert!(!diff.is_empty());
    }

    /// **CJ-05（B2）**：四类输入形状与控件事件都有结构化镜像，且序列化稳定。
    #[test]
    fn scene_event_records_keep_structure_and_serialize_stably() {
        use ze_notion::Vec2;
        use ze_scene_core::domain::SceneEvent;
        use ze_scene_core::domain::event::{
            CommandInput, KeyInput, KeyModifiers, PointerInput, TextInput,
        };

        let events = vec![
            SceneEvent::Toggle {
                id: ze_scene_core::domain::ids::PLAY_PAUSE,
                value: true,
            },
            SceneEvent::Key {
                input: KeyInput::Down {
                    key: "Enter".into(),
                    modifiers: KeyModifiers {
                        ctrl: true,
                        ..KeyModifiers::default()
                    },
                },
            },
            SceneEvent::Text {
                input: TextInput::Commit {
                    text: "hello".into(),
                },
            },
            SceneEvent::Pointer {
                input: PointerInput::Wheel {
                    scene_pos: Vec2::new(1.0, 2.0),
                    delta: Vec2::new(0.0, -3.0),
                },
            },
            SceneEvent::Command {
                input: CommandInput::Invoke {
                    name: "panel_a".into(),
                },
            },
        ];
        let records: Vec<SceneEventRecord> = events.iter().map(SceneEventRecord::from).collect();
        let first = serde_json::to_string(&records).expect("可序列化");
        let second = serde_json::to_string(&records).expect("可序列化");
        assert_eq!(first, second, "两次序列化必须逐字节相同");
        for kind in [
            "toggle",
            "key_down",
            "text_commit",
            "pointer_wheel",
            "command_invoke",
        ] {
            assert!(first.contains(kind), "缺结构化变体 `{kind}`：{first}");
        }
        assert!(
            !first.contains("debug"),
            "不得再落进 Debug 字符串兜底：{first}"
        );
        let back: Vec<SceneEventRecord> = serde_json::from_str(&first).expect("可反序列化");
        assert_eq!(records, back);
    }
}
