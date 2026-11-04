//! Pure geometry types shared by the editor state machine and the UI layer.
//!
//! All coordinates are **graph local**: origin at the graph's top-left,
//! y-axis pointing down, 1:1 with pixels. Viewport transforms (pan/zoom) live
//! exclusively in the UI layer; this crate never sees screen coordinates.
//!
//! # 为什么不用 `glam::Vec2`
//!
//! 这里的二维值**不是数学向量**，是「节点卡 / 插脚在图里的位置」。用专门的
//! [`NodePos`] 承载它：语义靠类型名表达，且不把渲染侧的向量库引进来
//! （节点栈因此自包含，门禁 `JQ_GLAM` 覆盖本 crate）。

use ze_node_core::{EdgeId, InputSocketId, NodeId, OutputSocketId};

/// 图坐标里的一个位置（`[x, y]`，原点在图左上、y 向下）。
///
/// 只提供几何运算所需的最小集合（加 / 减 / 数乘 / 点积 / 长度 / 线性插值）。
/// 需要更多线性代数时用 `ze-notion` 的 `Vec2`，不要在这里扩。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NodePos {
    pub x: f32,
    pub y: f32,
}

impl NodePos {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn splat(value: f32) -> Self {
        Self {
            x: value,
            y: value,
        }
    }

    pub fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// 线性插值：`t = 0` ⇒ `from`，`t = 1` ⇒ `to`。
    pub fn lerp(from: Self, to: Self, t: f32) -> Self {
        from + (to - from) * t
    }

    /// 逐分量最小值（包围盒用）。
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// 逐分量最大值（包围盒用）。
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }
}

impl From<(f32, f32)> for NodePos {
    fn from(value: (f32, f32)) -> Self {
        Self::new(value.0, value.1)
    }
}

impl From<[f32; 2]> for NodePos {
    fn from(value: [f32; 2]) -> Self {
        Self::new(value[0], value[1])
    }
}

impl From<NodePos> for [f32; 2] {
    fn from(value: NodePos) -> Self {
        [value.x, value.y]
    }
}

impl From<NodePos> for (f32, f32) {
    fn from(value: NodePos) -> Self {
        (value.x, value.y)
    }
}

impl std::ops::Add for NodePos {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::AddAssign for NodePos {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub for NodePos {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::SubAssign for NodePos {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl std::ops::Mul<f32> for NodePos {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl std::ops::Div<f32> for NodePos {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl std::ops::Mul<NodePos> for f32 {
    type Output = NodePos;
    fn mul(self, rhs: NodePos) -> NodePos {
        rhs * self
    }
}

/// Axis-aligned rectangle in graph local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: NodePos,
    pub max: NodePos,
}

impl Rect {
    /// Build from a top-left corner and a (positive) size.
    pub fn from_min_size(min: NodePos, size: NodePos) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    /// Width / height.
    pub fn size(self) -> NodePos {
        self.max - self.min
    }

    /// Center point.
    pub fn center(self) -> NodePos {
        (self.min + self.max) * 0.5
    }

    /// 投影到交互层（`ze-interact`）：命中判定与场景面板共用同一条口径。
    pub fn to_interact(self) -> ze_interact::Rect {
        ze_interact::Rect::new(interact_point(self.min), interact_point(self.max))
    }

    /// Point-in-rect test.
    ///
    /// **半开区间 `[min, max)`**（与 `ze_interact::Rect::contains` 同一实现）：
    /// 两张卡贴边时，共享边只属于后一张 —— 命中结果唯一，不随遍历顺序变化。
    pub fn contains(self, p: NodePos) -> bool {
        self.to_interact().contains(interact_point(p))
    }

    /// Grow (or shrink, for negative `r`) the rectangle by `r` on every side.
    /// Used to add hit-test tolerance.
    pub fn expand(self, r: f32) -> Self {
        Self {
            min: self.min - NodePos::splat(r),
            max: self.max + NodePos::splat(r),
        }
    }
}

/// 图坐标点 → 交互层点（`f32` → `f64`，边界唯一）。
pub fn interact_point(p: NodePos) -> ze_interact::Point {
    ze_interact::Point::from_f32(p.x, p.y)
}

/// Result of a hit test. Variants are ordered by z-priority: sockets are
/// tested before resize grips, grips before cards, cards before edges;
/// callers match top-down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HitTarget {
    Node(NodeId),
    InputSocket(InputSocketId),
    OutputSocket(OutputSocketId),
    /// The bottom-right resize grip of a node card.
    ResizeHandle(NodeId),
    Edge(EdgeId),
    Nothing,
}

/// Hit radius (graph-local units / pixels at zoom 1.0) for socket picking.
/// Generous on purpose: the on-screen dot is small, but clicks anywhere in
/// this disk grab the pin.
pub const SOCKET_HIT_RADIUS: f32 = 12.0;

/// Snap radius (graph-local units): while dragging a connection, the cursor
/// is attracted to the nearest type-compatible input socket within this
/// distance (edge endpoint snaps to the pin and releasing completes the
/// connection even if the pointer is slightly off).
pub const SOCKET_SNAP_RADIUS: f32 = 22.0;

/// Hit radius (graph-local units) for the bottom-right resize grip.
pub const RESIZE_HIT_RADIUS: f32 = 14.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_basic() {
        let r = Rect::from_min_size((0.0, 0.0).into(), (10.0, 20.0).into());
        assert!(r.contains((5.0, 5.0).into()));
        assert!(!r.contains((11.0, 5.0).into()));
    }

    #[test]
    fn rect_size_and_center() {
        let r = Rect::from_min_size((0.0, 0.0).into(), (10.0, 20.0).into());
        assert_eq!(r.size(), NodePos::new(10.0, 20.0));
        assert_eq!(r.center(), NodePos::new(5.0, 10.0));
    }

    #[test]
    fn rect_expand_adds_tolerance() {
        let r = Rect::from_min_size((0.0, 0.0).into(), (10.0, 10.0).into());
        let p = (12.0, 5.0).into();
        assert!(!r.contains(p));
        assert!(r.expand(3.0).contains(p));
    }

    /// 与场景交互层同一条半开口径：`min` 属于本矩形，`max` 属于下一个。
    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect::from_min_size((0.0, 0.0).into(), (10.0, 10.0).into());
        assert!(r.contains((0.0, 0.0).into()), "min 端点属于本矩形");
        assert!(r.contains((9.999, 9.999).into()));
        assert!(!r.contains((10.0, 10.0).into()), "max 端点属于下一个");
    }

    /// 贴边的两张卡不会同时命中（命中唯一）。
    #[test]
    fn adjacent_cards_do_not_double_hit_at_the_shared_edge() {
        let left = Rect::from_min_size((0.0, 0.0).into(), (10.0, 10.0).into());
        let right = Rect::from_min_size((10.0, 0.0).into(), (10.0, 10.0).into());
        let edge = (10.0, 5.0).into();
        assert!(!left.contains(edge));
        assert!(right.contains(edge));
    }

    #[test]
    fn node_pos_arithmetic_and_roundtrip() {
        let a = NodePos::new(1.0, 2.0);
        let b = NodePos::from([3.0, 4.0]);
        assert_eq!(a + b, NodePos::new(4.0, 6.0));
        assert_eq!(b - a, NodePos::new(2.0, 2.0));
        assert_eq!(a * 2.0, NodePos::new(2.0, 4.0));
        assert_eq!(2.0 * a, NodePos::new(2.0, 4.0));
        assert_eq!(NodePos::lerp(a, b, 0.5), NodePos::new(2.0, 3.0));
        assert_eq!((a + b).length_squared(), 16.0 + 36.0);
        assert_eq!(<[f32; 2]>::from(a), [1.0, 2.0]);
        assert_eq!(<(f32, f32)>::from(a), (1.0, 2.0));
    }
}
