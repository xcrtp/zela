//! 命中测试（CE-a）：矩形优先 + 索引接口预留。
//!
//! # 为什么矩形是闭区间以外的半开区间
//!
//! `[min, max)`：相邻控件贴边时，边界点**只属于后一个**。若用闭区间，一条分隔线
//! 会被两个控件同时命中 —— 结果是「点谁都能改」而不是「点谁改谁」。
//! 半开口径让命中结果唯一，且与遍历顺序无关（确定性）。
//!
//! # 为什么 z-order 降序 + 同 z 按插入顺序
//!
//! 「谁在前面就先命中谁」是本 crate 唯一认的层次语义（不做深度模型）。
//! 同 z 时后插入者在上（与场景声明顺序一致）⇒ 排序必须**稳定**。

/// 交互坐标点（内部口径 f64）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// 边界入口：渲染空间（f32）→ 内部 f64。
    pub fn from_f32(x: f32, y: f32) -> Self {
        Self {
            x: f64::from(x),
            y: f64::from(y),
        }
    }

    /// 边界出口：内部 f64 → 渲染空间（f32）。
    pub fn to_f32(self) -> (f32, f32) {
        (self.x as f32, self.y as f32)
    }
}

/// 轴对齐矩形（构造时规范化：`min ≤ max`）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    /// 构造并规范化（颠倒的端点会被交换，不静默丢弃）。
    pub fn new(a: Point, b: Point) -> Self {
        Self {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// 左上角 + 尺寸。
    pub fn from_min_size(min: Point, width: f64, height: f64) -> Self {
        Self::new(min, Point::new(min.x + width, min.y + height))
    }

    pub fn center(self) -> Point {
        Point::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn is_empty(self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// 半开区间包含判定 `[min, max)`（见模块文档）。
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }

    /// 两矩形是否相交（半开口径：贴边不算相交）。
    pub fn intersects(self, other: Rect) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }
}

/// 命中目标的稳定 id（由宿主分配；与场景 `ObjectId` 解耦）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct HitId(pub u32);

/// 命中区域。
///
/// CE 只有矩形；曲线（贝塞尔）走 [`CurveHitTester`] 接口，实现留后续批次。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitRegion {
    Rect(Rect),
}

impl HitRegion {
    pub fn contains(self, point: Point) -> bool {
        match self {
            Self::Rect(rect) => rect.contains(point),
        }
    }
}

/// 一个可命中目标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitTarget {
    pub id: HitId,
    pub region: HitRegion,
    /// 层次：**越大越靠前**（先被命中）。
    pub z: i32,
}

impl HitTarget {
    pub fn rect(id: HitId, rect: Rect, z: i32) -> Self {
        Self {
            id,
            region: HitRegion::Rect(rect),
            z,
        }
    }
}

/// 命中索引接口（CE-a 预留：后续可换成网格 / BVH，不改调用方）。
pub trait HitIndex {
    /// 返回该点命中的目标，按 z-order 降序（同 z 按插入顺序）。
    fn query(&self, point: Point) -> Vec<HitId>;
}

/// **CK-e**：命中链上「消费还是继续」的显式规则。
///
/// `query` 返回的 `Vec` 天然是命中链（前 → 后 / 内 → 外）。冒泡到容器就是
/// 对链上后续项 `Continue`；子控件处理掉事件就是 `Consume`。默认不是"谁都收到"，
/// 也不是"只给第一个" —— 规则必须由消费方写出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitFlow {
    /// 已处理，停止冒泡。
    Consume,
    /// 未处理，继续给链上后一个（通常是容器 / 祖先）。
    Continue,
}

/// 命中链：按命中顺序排列的目标。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HitChain {
    hits: Vec<HitId>,
}

impl HitChain {
    pub fn new(hits: Vec<HitId>) -> Self {
        Self { hits }
    }

    pub fn from_index(index: &dyn HitIndex, point: Point) -> Self {
        Self::new(index.query(point))
    }

    pub fn as_slice(&self) -> &[HitId] {
        &self.hits
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// 沿链冒泡，返回**消费**该事件的目标；全部 `Continue` ⇒ `None`。
    ///
    /// 这是「消费 / 继续」的唯一实现：调用方不再各自写"谁先谁后"。
    pub fn visit(&self, mut visit: impl FnMut(HitId) -> HitFlow) -> Option<HitId> {
        for &hit in &self.hits {
            if visit(hit) == HitFlow::Consume {
                return Some(hit);
            }
        }
        None
    }
}

/// 线性索引：CE 的唯一实现（目标少，O(n) 足够；换实现不改调用方）。
#[derive(Debug, Clone, Default)]
pub struct LinearHitIndex {
    /// 已按 z 降序、同 z 按插入顺序排好。
    targets: Vec<HitTarget>,
}

impl LinearHitIndex {
    /// 构造：排序是**稳定**的（同 z 保持调用方给的顺序）。
    pub fn new(targets: Vec<HitTarget>) -> Self {
        let mut targets = targets;
        targets.sort_by(|a, b| b.z.cmp(&a.z));
        Self { targets }
    }

    pub fn targets(&self) -> &[HitTarget] {
        &self.targets
    }
}

impl HitIndex for LinearHitIndex {
    fn query(&self, point: Point) -> Vec<HitId> {
        self.targets
            .iter()
            .filter(|target| target.region.contains(point))
            .map(|target| target.id)
            .collect()
    }
}

/// 曲线命中接口（**只留签名**，实现留后续批次）。
///
/// 现有贝塞尔求交实现在场景层且依赖 glam；直接搬进本 crate 会破坏零依赖约束。
/// 需要曲线的批次再评估：要么在这里重写成 f64 实现，要么由宿主注入实现。
pub trait CurveHitTester {
    /// 折线 / 曲线展平后的点表是否在 `tolerance` 内被 `point` 命中。
    fn hit_polyline(&self, points: &[Point], point: Point, tolerance: f64) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::new(Point::new(x0, y0), Point::new(x1, y1))
    }

    #[test]
    fn rect_normalizes_and_contains_half_open() {
        let r = rect(10.0, 10.0, 30.0, 20.0);
        assert_eq!(r.min, Point::new(10.0, 10.0));
        assert_eq!(r.max, Point::new(30.0, 20.0));
        assert!(r.contains(Point::new(10.0, 10.0)), "min 端点属于本矩形");
        assert!(!r.contains(Point::new(30.0, 20.0)), "max 端点属于下一个");
        assert!(r.contains(Point::new(29.999, 19.999)));
        assert!(!r.contains(Point::new(9.999, 15.0)));

        // 颠倒端点：构造时规范化，不静默丢。
        let flipped = Rect::new(Point::new(30.0, 20.0), Point::new(10.0, 10.0));
        assert_eq!(flipped, r);
    }

    #[test]
    fn adjacent_rects_do_not_double_hit_at_the_shared_edge() {
        let left = rect(0.0, 0.0, 10.0, 10.0);
        let right = rect(10.0, 0.0, 20.0, 10.0);
        let edge = Point::new(10.0, 5.0);
        assert!(!left.contains(edge));
        assert!(right.contains(edge), "共享边只属于后一个矩形");
        assert!(!left.intersects(right), "贴边不算相交");
    }

    #[test]
    fn hit_index_is_z_descending_and_stable() {
        let index = LinearHitIndex::new(vec![
            HitTarget::rect(HitId(1), rect(0.0, 0.0, 100.0, 100.0), 0),
            HitTarget::rect(HitId(2), rect(0.0, 0.0, 50.0, 50.0), 5),
            HitTarget::rect(HitId(3), rect(0.0, 0.0, 50.0, 50.0), 5),
            HitTarget::rect(HitId(4), rect(200.0, 200.0, 300.0, 300.0), 9),
        ]);
        // 同 z（2 / 3）保持插入顺序（稳定排序）。
        assert_eq!(
            index.query(Point::new(25.0, 25.0)),
            vec![HitId(2), HitId(3), HitId(1)]
        );
        assert_eq!(index.query(Point::new(75.0, 75.0)), vec![HitId(1)]);
        assert!(index.query(Point::new(400.0, 400.0)).is_empty());
    }

    #[test]
    fn point_boundary_roundtrips_through_f32() {
        let p = Point::from_f32(1.5, -2.25);
        assert_eq!(p, Point::new(1.5, -2.25));
        assert_eq!(p.to_f32(), (1.5, -2.25));
    }

    /// **CK-e**：链上显式「消费 / 继续」—— 子控件消费就停，否则冒泡到容器。
    #[test]
    fn hit_chain_bubbles_until_a_target_consumes() {
        let chain = HitChain::new(vec![HitId(2), HitId(1), HitId(0)]);
        // 2 不处理（例如装饰层），1 处理 ⇒ 停在 1，0 收不到。
        let mut seen = Vec::new();
        let consumer = chain.visit(|hit| {
            seen.push(hit);
            if hit == HitId(1) {
                HitFlow::Consume
            } else {
                HitFlow::Continue
            }
        });
        assert_eq!(consumer, Some(HitId(1)));
        assert_eq!(seen, vec![HitId(2), HitId(1)], "消费后不再冒泡");

        // 全部继续 ⇒ 没有消费者（调用方可据此判定"未处理"）。
        let none = chain.visit(|_| HitFlow::Continue);
        assert_eq!(none, None);
        assert!(HitChain::default().is_empty());
    }
}
