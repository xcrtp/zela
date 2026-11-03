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
use ze_scene_core::domain::{InjectError, InjectSource, TagId};

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

/// 派生态诊断：命中谁 / 路由去向 / 为什么落空。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    /// capture > focus > hover 的当前投递目标（语义名）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed: Option<String>,
    /// 没有可观察帧时的说明（例如纯 3D 场景）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
