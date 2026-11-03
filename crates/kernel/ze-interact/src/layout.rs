//! 布局（CE-b）：盒子 + 流式，**纯函数、无求解器**。
//!
//! # 为什么不引约束求解器（kasuari）
//!
//! 参数面板就是**垂直列表**，用不上约束。现在引 = 零消费者依赖 ——
//! 本项目最忌的形态。等思维导图那批再评估。
//!
//! # 为什么只取 `LayoutStrategy` 的形状、不搬实现
//!
//! 节点栈那套是 `glam::Vec2` + `HashMap<NodeId, Vec2>`（"松的那套"）。
//! 本 crate 只沿用它的**两个约定**：
//!
//! 1. `layout(constraints) -> Vec<PlacedRect>`；
//! 2. `auto_layout_supported()` —— `false` 时**自动布局不得覆盖用户手选位置**。
//!
//! 第二条与 CC 的写仲裁同型（"自动布局不得覆盖用户手选" ≈ "动画不得覆盖用户拖动"）：
//! 判定放在 [`apply_layout`] 里，一处实现。
//!
//! # 硬约束：布局只在**结构变化时**算
//!
//! 控件增删 / 文本变化才重算，**不在每帧**。与"文档不进场景"同一思路：
//! 静态的东西不要放进每帧循环。调用方负责缓存结果（本 crate 无状态）。

use crate::hit::{Point, Rect};

/// 布局中的控件 id（由宿主分配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LayoutId(pub u32);

/// 尺寸（内部口径 f64）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// 可用空间约束（CE 只有上界：参数面板是"给多少宽、往下排"）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Constraints {
    pub max: Size,
}

impl Constraints {
    pub const fn new(max_width: f64, max_height: f64) -> Self {
        Self {
            max: Size::new(max_width, max_height),
        }
    }
}

/// 一个待布局控件：id + 自然尺寸。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutItem {
    pub id: LayoutId,
    pub size: Size,
}

impl LayoutItem {
    pub const fn new(id: LayoutId, width: f64, height: f64) -> Self {
        Self {
            id,
            size: Size::new(width, height),
        }
    }
}

/// 布局结果：控件 id + 落点矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedRect {
    pub id: LayoutId,
    pub rect: Rect,
}

/// 布局策略（效仿节点栈的 `LayoutStrategy` 形状，实现重写）。
pub trait Layout {
    /// 按约束摆放全部控件（顺序 = 声明顺序）。
    fn layout(&self, constraints: Constraints) -> Vec<PlacedRect>;

    /// 是否支持自动布局。`false` ⇒ [`apply_layout`] 不得覆盖用户手选位置。
    fn auto_layout_supported(&self) -> bool {
        true
    }
}

/// 盒子布局：**垂直**堆叠（参数面板的唯一形态）。
#[derive(Debug, Clone, Default)]
pub struct BoxLayout {
    pub items: Vec<LayoutItem>,
    /// 行间距。
    pub gap: f64,
    /// 内边距（四边同宽）。
    pub padding: f64,
}

impl BoxLayout {
    pub fn new(items: Vec<LayoutItem>, gap: f64, padding: f64) -> Self {
        Self {
            items,
            gap,
            padding,
        }
    }
}

impl Layout for BoxLayout {
    fn layout(&self, constraints: Constraints) -> Vec<PlacedRect> {
        let mut out = Vec::with_capacity(self.items.len());
        let mut y = self.padding;
        for item in &self.items {
            let width = item
                .size
                .width
                .min((constraints.max.width - self.padding * 2.0).max(0.0));
            out.push(PlacedRect {
                id: item.id,
                rect: Rect::from_min_size(Point::new(self.padding, y), width, item.size.height),
            });
            y += item.size.height + self.gap;
        }
        out
    }
}

/// 流式布局：**水平**排列，超出可用宽度换行。
#[derive(Debug, Clone, Default)]
pub struct FlowLayout {
    pub items: Vec<LayoutItem>,
    pub gap: f64,
    pub padding: f64,
}

impl FlowLayout {
    pub fn new(items: Vec<LayoutItem>, gap: f64, padding: f64) -> Self {
        Self {
            items,
            gap,
            padding,
        }
    }
}

impl Layout for FlowLayout {
    fn layout(&self, constraints: Constraints) -> Vec<PlacedRect> {
        let mut out = Vec::with_capacity(self.items.len());
        let limit = (constraints.max.width - self.padding).max(0.0);
        let mut x = self.padding;
        let mut y = self.padding;
        let mut row_height: f64 = 0.0;
        for item in &self.items {
            if x > self.padding && x + item.size.width > limit {
                x = self.padding;
                y += row_height + self.gap;
                row_height = 0.0;
            }
            out.push(PlacedRect {
                id: item.id,
                rect: Rect::from_min_size(Point::new(x, y), item.size.width, item.size.height),
            });
            x += item.size.width + self.gap;
            row_height = row_height.max(item.size.height);
        }
        out
    }
}

/// 自动布局与**用户手选位置**的仲裁（一处实现）。
///
/// - `auto_layout_supported() == false` ⇒ 原样返回 `existing`（自动布局不覆盖手选）；
/// - 否则 ⇒ 手选优先，没有手选的控件取自动布局结果。
///
/// 返回长度 = `existing.len().max(自动布局结果长度)`，缺失项为 `None`
/// （调用方据此决定是否拒绝 / 记录，不静默丢控件）。
pub fn apply_layout(
    layout: &dyn Layout,
    constraints: Constraints,
    existing: &[Option<Rect>],
) -> Vec<Option<Rect>> {
    if !layout.auto_layout_supported() {
        return existing.to_vec();
    }
    let auto = layout.layout(constraints);
    let len = existing.len().max(auto.len());
    (0..len)
        .map(|index| {
            existing
                .get(index)
                .copied()
                .flatten()
                .or_else(|| auto.get(index).map(|placed| placed.rect))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(heights: &[f64]) -> Vec<LayoutItem> {
        heights
            .iter()
            .enumerate()
            .map(|(index, height)| LayoutItem::new(LayoutId(index as u32), 100.0, *height))
            .collect()
    }

    #[test]
    fn box_layout_stacks_vertically_with_gap_and_padding() {
        let layout = BoxLayout::new(items(&[20.0, 30.0]), 4.0, 8.0);
        let placed = layout.layout(Constraints::new(120.0, 200.0));
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].rect.min, Point::new(8.0, 8.0));
        assert_eq!(placed[0].rect.max, Point::new(108.0, 28.0));
        assert_eq!(placed[1].rect.min, Point::new(8.0, 32.0));
        assert_eq!(placed[1].rect.max, Point::new(108.0, 62.0));
    }

    #[test]
    fn flow_layout_wraps_at_the_available_width() {
        let layout = FlowLayout::new(
            vec![
                LayoutItem::new(LayoutId(0), 40.0, 10.0),
                LayoutItem::new(LayoutId(1), 40.0, 10.0),
                LayoutItem::new(LayoutId(2), 40.0, 10.0),
            ],
            5.0,
            0.0,
        );
        let placed = layout.layout(Constraints::new(90.0, 100.0));
        // 0 与 1 在第一行（40 + 5 + 40 = 85 ≤ 90），2 换行。
        assert_eq!(placed[0].rect.min, Point::new(0.0, 0.0));
        assert_eq!(placed[1].rect.min, Point::new(45.0, 0.0));
        assert_eq!(placed[2].rect.min, Point::new(0.0, 15.0));
    }

    /// 不支持自动布局的策略**不得覆盖**用户手选位置。
    #[test]
    fn auto_layout_disabled_keeps_manual_positions() {
        struct ManualOnly(BoxLayout);
        impl Layout for ManualOnly {
            fn layout(&self, constraints: Constraints) -> Vec<PlacedRect> {
                self.0.layout(constraints)
            }
            fn auto_layout_supported(&self) -> bool {
                false
            }
        }

        let manual = Rect::from_min_size(Point::new(300.0, 50.0), 10.0, 10.0);
        let layout = ManualOnly(BoxLayout::new(items(&[20.0, 20.0]), 0.0, 0.0));
        let resolved = apply_layout(
            &layout,
            Constraints::new(500.0, 500.0),
            &[Some(manual), None],
        );
        assert_eq!(resolved[0], Some(manual), "手选位置必须原样保留");
        assert_eq!(resolved[1], None, "自动布局不得填补");
    }

    /// 支持自动布局时：手选优先，其余取自动结果。
    #[test]
    fn auto_layout_fills_only_unplaced_items() {
        let manual = Rect::from_min_size(Point::new(300.0, 50.0), 10.0, 10.0);
        let layout = BoxLayout::new(items(&[20.0, 20.0]), 0.0, 0.0);
        let resolved = apply_layout(
            &layout,
            Constraints::new(500.0, 500.0),
            &[Some(manual), None],
        );
        assert_eq!(resolved[0], Some(manual));
        assert_eq!(
            resolved[1],
            Some(Rect::from_min_size(Point::new(0.0, 20.0), 100.0, 20.0))
        );
    }
}
