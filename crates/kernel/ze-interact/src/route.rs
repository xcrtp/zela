//! 路由（CE-c）：hover / focus / capture 三档。
//!
//! # 三档的语义
//!
//! | 档 | 何时建立 | 何时清除 |
//! |---|---|---|
//! | **hover** | 每帧按指针位置重算 | 指针移出 / 无命中 |
//! | **focus** | 按下命中目标 | 下一次按到别处 / 按到空白 |
//! | **capture** | 按下命中目标（拖拽锁定） | 抬起 |
//!
//! # 为什么 capture 不跟命中走
//!
//! 拖拽时指针很容易移出滑块 —— 若按命中结果走，拖到一半就"丢抓"。
//! capture 就是「一段时间内**故意不按命中结果走**」，因此它是**唯一**的
//! 确定性风险点。处置：
//!
//! - 路由状态**不进产物**（本模块的类型不派生 serde）；
//! - 重放同一事件流 ⇒ 同一状态（纯函数式状态机，没有时间 / 随机来源）。
//!
//! # 路由键
//!
//! `scope`（CC 的命名空间路径）+ `HitId`。两个面板里各有一个"第 0 号滑块"时，
//! 只有带上 scope 才能区分 —— 这是 CC 的作用域在交互层的第一个消费者。

/// 路由键：作用域路径 + 命中目标。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteKey {
    /// CC 的作用域路径（根作用域为空串）。
    pub scope: String,
    /// 命中目标（[`crate::hit::HitId`]）。
    pub target: crate::hit::HitId,
}

impl RouteKey {
    pub fn new(scope: impl Into<String>, target: crate::hit::HitId) -> Self {
        Self {
            scope: scope.into(),
            target,
        }
    }
}

/// 当前档位（诊断 / 测试用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePhase {
    Hover,
    Focus,
    Capture,
}

/// 路由状态机（无时间 / 无随机来源 ⇒ 事件流相同则状态相同）。
#[derive(Debug, Clone, Default)]
pub struct Router {
    hover: Option<RouteKey>,
    focus: Option<RouteKey>,
    capture: Option<RouteKey>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// 指针移动：更新 hover。**capture 期间不改投递目标**。
    pub fn pointer_moved(&mut self, hit: Option<RouteKey>) {
        self.hover = hit;
    }

    /// 按下：建立 focus 与 capture。按到空白 ⇒ 两者都清空（焦点不悬在别处）。
    pub fn pressed(&mut self, hit: Option<RouteKey>) {
        self.focus = hit.clone();
        self.capture = hit;
    }

    /// 抬起：解除 capture（focus 保持到下一次按下）。
    pub fn released(&mut self) {
        self.capture = None;
    }

    /// 清空全部状态（宿主切场景 / 失去焦点时用）。
    pub fn cancel(&mut self) {
        self.hover = None;
        self.focus = None;
        self.capture = None;
    }

    pub fn hover(&self) -> Option<&RouteKey> {
        self.hover.as_ref()
    }

    pub fn focus(&self) -> Option<&RouteKey> {
        self.focus.as_ref()
    }

    pub fn capture(&self) -> Option<&RouteKey> {
        self.capture.as_ref()
    }

    /// 当前档位：capture > focus > hover。
    pub fn phase(&self) -> Option<RoutePhase> {
        if self.capture.is_some() {
            Some(RoutePhase::Capture)
        } else if self.focus.is_some() {
            Some(RoutePhase::Focus)
        } else if self.hover.is_some() {
            Some(RoutePhase::Hover)
        } else {
            None
        }
    }

    /// 事件该发给谁：capture > focus > hover。
    pub fn dispatch(&self) -> Option<&RouteKey> {
        self.capture
            .as_ref()
            .or(self.focus.as_ref())
            .or(self.hover.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit::{HitId, Point, Rect, HitRegion};

    fn key(scope: &str, id: u32) -> RouteKey {
        RouteKey::new(scope, HitId(id))
    }

    fn rect_hit(x0: f64, x1: f64) -> HitRegion {
        HitRegion::Rect(Rect::new(Point::new(x0, 0.0), Point::new(x1, 10.0)))
    }

    #[test]
    fn hover_tracks_the_pointer() {
        let mut router = Router::new();
        router.pointer_moved(Some(key("panel_a", 1)));
        assert_eq!(router.hover(), Some(&key("panel_a", 1)));
        assert_eq!(router.phase(), Some(RoutePhase::Hover));
        assert_eq!(router.dispatch(), Some(&key("panel_a", 1)));

        router.pointer_moved(None);
        assert_eq!(router.phase(), None);
        assert_eq!(router.dispatch(), None);
    }

    #[test]
    fn focus_survives_pointer_leaving_until_the_next_press() {
        let mut router = Router::new();
        router.pressed(Some(key("panel_a", 1)));
        router.released();
        router.pointer_moved(None);
        assert_eq!(router.focus(), Some(&key("panel_a", 1)));
        assert_eq!(router.dispatch(), Some(&key("panel_a", 1)), "focus 优先于 hover");

        router.pressed(Some(key("panel_b", 2)));
        assert_eq!(router.focus(), Some(&key("panel_b", 2)));

        router.released();
        router.pressed(None);
        assert_eq!(router.focus(), None, "按到空白 ⇒ 焦点清空");
    }

    /// **CE-c 关键用例**：拖拽期间指针移出，capture 仍然生效。
    #[test]
    fn capture_wins_even_when_the_pointer_moves_out() {
        let mut router = Router::new();
        router.pointer_moved(Some(key("panel_a", 1)));
        router.pressed(Some(key("panel_a", 1)));

        // 指针移到面板外（命中为空），甚至落到另一个面板上。
        router.pointer_moved(None);
        assert_eq!(router.dispatch(), Some(&key("panel_a", 1)), "移出仍锁定");
        router.pointer_moved(Some(key("panel_b", 2)));
        assert_eq!(
            router.dispatch(),
            Some(&key("panel_a", 1)),
            "拖拽期间不按命中结果走"
        );
        assert_eq!(router.phase(), Some(RoutePhase::Capture));

        router.released();
        assert_eq!(router.phase(), Some(RoutePhase::Focus));
        assert_eq!(router.capture(), None);
    }

    /// 两个作用域的同号目标必须能区分（CC 命名空间是路由键的一半）。
    #[test]
    fn scope_is_part_of_the_route_key() {
        let mut router = Router::new();
        router.pressed(Some(key("panel_a", 0)));
        router.pointer_moved(Some(key("panel_b", 0)));
        let dispatched = router.dispatch().expect("有投递目标");
        assert_eq!(dispatched.scope, "panel_a");
        assert_ne!(dispatched, &key("panel_b", 0));
    }

    /// 路由状态不参与产物：本模块类型不派生 serde（结构性约束）。
    #[test]
    fn hit_regions_are_used_for_hover_targets() {
        let region = rect_hit(0.0, 10.0);
        assert!(region.contains(Point::new(5.0, 5.0)));
        assert!(!region.contains(Point::new(15.0, 5.0)));
    }
}
