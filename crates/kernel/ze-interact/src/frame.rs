//! **CG-c**：结构化帧（`Frame`）—— UI 的一次**快照**。
//!
//! # 它像什么，不像什么
//!
//! 像的是**浏览器的某一帧**，不是浏览器本身。浏览器最贵的不是 DOM，是 DOM 的
//! **可变性**（增量重排、reflow、布局抖动、脚本随时改树）；我们这条路从第一天起是：
//!
//! ```text
//! 声明式输入 → 纯函数 → 完整帧描述（一个新值）
//! ```
//!
//! 更像 React 的 render 函数 / 立即模式 GUI 的函数式版，多一步「把帧导出成数据」。
//! 纯值、可哈希、可 seek、可丢弃重算 —— 浏览器花三十年治的「读一个属性触发同步布局」，
//! 这里结构上就不存在。
//!
//! # 三条平行结构，不是一棵树
//!
//! ```text
//! Frame {
//!   geometry:  Vec<PlacedRect>,      // 布局输出（谁画在哪）
//!   semantics: Vec<SemanticNode>,    // role / name / value / bounds / focusable
//!   text:      Vec<TextRun>,         // 字符索引 ↔ 矩形
//! }
//! ```
//!
//! 语义**不塞进** `PlacedRect`：HTML 把内容语义与呈现焊死在一起，后来补 ARIA 补得很
//! 狼狈。我们从第一天就分开 —— 等控件铺开再分就是重写。
//!
//! # 纪律（本批最重要的反面约束）
//!
//! 1. **Frame 是快照，不是可变树**：不提供 `set_rect()` 之类的写入口；
//! 2. **不进 payload**：它是计算结果，可导出、可比较，但不进 ZSB。
//!    与「路由状态不进产物」不矛盾：路由是**派生态**（靠事件流重建），
//!    Frame 是**计算结果**（每帧重算的纯值）。
//! 3. **不持有句柄**：没有 `ObjectId` / 实体 id / 指针。`PlacedRect::id` 是**声明期的
//!    下标**（[`crate::layout::LayoutId`]），不是运行期句柄。
//!
//! # 语义锚点最小集：五个，不扩
//!
//! [`SemanticNode`] 只有 `role / name / value / bounds / focusable`。五个已覆盖
//! 无障碍树与 Agent 定位；表外不兜底（扩成 grab bag 就失去了"锚点"的意义）。

use crate::hit::Rect;
use crate::layout::{LayoutId, PlacedRect};
use crate::text::TextRun;

/// UI 的一帧（快照）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    /// 布局输出：控件 id（声明期下标）+ 落点矩形。
    pub geometry: Vec<PlacedRect>,
    /// **CK-d**：父子索引（与 `geometry` 同下标；`None` = 根）。
    ///
    /// 纯值、不持句柄、仍是快照：它让结构变化（拖进 / 拖出容器）可被差分，
    /// 而不是只表现为「Geometry 变了」。`Frame` 不进 payload，故 parent 也不进。
    pub parent: Vec<Option<LayoutId>>,
    /// 语义层：与几何**平行**，不是它的属性。
    pub semantics: Vec<SemanticNode>,
    /// 文本层：字符索引 ↔ 矩形。
    pub text: Vec<TextRun>,
}

/// 语义锚点（最小集：五个字段，不扩）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SemanticNode {
    /// 角色（`slider` / `button` / `label` …）。
    pub role: String,
    /// 可读名字（Agent 与读屏都靠它定位）。
    pub name: String,
    /// 当前值（可空：按钮没有值）。
    pub value: Option<String>,
    /// 语义范围（自己带，不从 `geometry` 里查）。
    pub bounds: Rect,
    /// 是否可获得焦点。
    pub focusable: bool,
}

impl SemanticNode {
    pub fn new(role: impl Into<String>, name: impl Into<String>, bounds: Rect) -> Self {
        Self {
            role: role.into(),
            name: name.into(),
            value: None,
            bounds,
            focusable: false,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

impl Frame {
    pub fn new() -> Self {
        Self::default()
    }

    /// 空帧（没有任何几何 / 语义 / 文本）。
    pub fn is_empty(&self) -> bool {
        self.geometry.is_empty() && self.semantics.is_empty() && self.text.is_empty()
    }

    /// 按 `role` + `name` 找语义锚点（Agent 的定位方式；没有全局字符串寻址）。
    pub fn semantic(&self, role: &str, name: &str) -> Option<&SemanticNode> {
        self.semantics
            .iter()
            .find(|node| node.role == role && node.name == name)
    }

    /// 语义锚点的当前值（Agent 断言的最短路径）。
    pub fn value_of(&self, role: &str, name: &str) -> Option<&str> {
        self.semantic(role, name)
            .and_then(|node| node.value.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit::Point;
    use crate::layout::LayoutId;

    fn sample_frame() -> Frame {
        let mut frame = Frame::new();
        frame.geometry.push(PlacedRect {
            id: LayoutId(0),
            rect: Rect::from_min_size(Point::new(0.0, 0.0), 100.0, 20.0),
        });
        frame.semantics.push(
            SemanticNode::new(
                "slider",
                "panel_a.base",
                Rect::from_min_size(Point::new(0.0, 0.0), 100.0, 20.0),
            )
            .with_value("0.75")
            .focusable(true),
        );
        let mut run = TextRun::new(12.0);
        run.push(0, 'A', Point::new(4.0, 4.0), 8.0);
        frame.text.push(run);
        frame
    }

    #[test]
    fn same_declaration_and_events_give_bitwise_identical_frames() {
        assert_eq!(sample_frame(), sample_frame());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn frame_serializes_to_json_without_runtime_handles() {
        let frame = sample_frame();
        let json = serde_json::to_string(&frame).expect("可序列化");
        for forbidden in ["ObjectId", "handle", "entity", "ptr"] {
            assert!(!json.contains(forbidden), "Frame 不得含 `{forbidden}`：{json}");
        }
        let back: Frame = serde_json::from_str(&json).expect("可反序列化");
        assert_eq!(frame, back, "JSON 往返必须逐位相同");
        assert_eq!(frame.value_of("slider", "panel_a.base"), Some("0.75"));
    }

    /// 语义与几何**平行**：改一侧不影响另一侧。
    #[test]
    fn semantics_and_geometry_are_parallel() {
        let mut frame = sample_frame();
        let geometry = frame.geometry.clone();
        frame.semantics[0].name = "renamed".to_string();
        frame.semantics[0].bounds = Rect::from_min_size(Point::new(9.0, 9.0), 1.0, 1.0);
        assert_eq!(frame.geometry, geometry, "改语义不动几何");

        let semantics = frame.semantics.clone();
        frame.geometry.clear();
        assert_eq!(frame.semantics, semantics, "改几何不动语义");
    }

    #[test]
    fn lookup_is_by_role_and_name() {
        let frame = sample_frame();
        assert!(frame.semantic("slider", "panel_a.base").is_some());
        assert!(frame.semantic("slider", "missing").is_none());
        assert!(frame.semantic("button", "panel_a.base").is_none());
        assert!(frame.value_of("button", "panel_a.base").is_none());
    }
}
