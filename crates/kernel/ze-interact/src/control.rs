//! **CG-d**：控件层 —— 从「画控件」变成「描述控件」。
//!
//! ```text
//! ControlSpec（类型 / 范围 / 当前值 / 语义名）
//!    → 事件驱动状态机（hover / focus / capture 复用 [`crate::route::Router`]）
//!    → 输出几何（[`crate::layout::PlacedRect`]）+ 语义（[`crate::frame::SemanticNode`]）
//! ```
//!
//! # 为什么不在这一层画
//!
//! 控件的**形状**（矩形 / 圆 / 文字轮廓）由宿主决定；本模块只给"摆在哪、叫什么、
//! 值是多少、点下去会发生什么"。宿主据此生成场景图元（CG-f 的门面）或直接读帧
//! （CG-g 的 Agent 路径）—— 同一份描述，两个消费者。
//!
//! # 布局不在每帧
//!
//! [`ControlSet::frame`] 是**纯函数**：同样的规格 + 同样的约束 ⇒ 同样的帧。
//! 宿主只在**结构变化**（控件增删 / 文本变化 / 视口尺寸变化）时调用它，
//! 不要放进每帧循环（本 crate 无状态，缓存由宿主负责；见 CG-e 的对拍用例）。

use std::collections::BTreeSet;

use crate::frame::{Frame, SemanticNode};
use crate::hit::{HitId, Point, Rect};
use crate::layout::{BoxLayout, Constraints, Layout, LayoutId, LayoutItem, PlacedRect};
use crate::route::{RouteKey, Router};

/// 控件类型（本批四种：滑块 / 按钮 / 标签 / 单选组）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlKind {
    /// 水平滑块：`[min, max]` 的连续值。
    Slider { min: f64, max: f64 },
    /// 按钮：按下与抬起都在同一控件内才算激活。
    Button,
    /// 纯标签：不可交互。
    Label,
    /// **CI-b**：单选组 —— 值 = 选项下标（`0..options.len()`）。
    ///
    /// 形状只为「图层 / 相机开关」这类枚举常量；选项文本是**静态**的
    /// （`&'static [&'static str]`），因此 `ControlKind` 仍是 `Copy`。
    RadioGroup { options: &'static [&'static str] },
}

impl ControlKind {
    /// 自然尺寸（宽按可用宽度铺满；高度按控件类型）。
    ///
    /// `pub(crate)`：`widget::BuiltinWidget::measure` 必须与它逐位一致，
    /// 否则命中矩形（本状态机）与 `Frame`（`ui_layout`）会错位。
    pub(crate) fn height(self) -> f64 {
        match self {
            ControlKind::Slider { .. } => 20.0,
            ControlKind::Button => 24.0,
            ControlKind::Label => 16.0,
            ControlKind::RadioGroup { .. } => 20.0,
        }
    }

    fn interactive(self) -> bool {
        !matches!(self, ControlKind::Label)
    }
}

/// 控件声明（与 [`crate::frame::SemanticNode`] 的五个锚点一一对应）。
#[derive(Debug, Clone, PartialEq)]
pub struct ControlSpec {
    pub id: LayoutId,
    /// 语义名（Agent 与读屏都靠它定位；同一组里必须唯一）。
    pub name: String,
    pub kind: ControlKind,
    /// 当前值：滑块是数值；按钮 / 标签没有。
    pub value: Option<f64>,
}

impl ControlSpec {
    pub fn slider(id: LayoutId, name: impl Into<String>, min: f64, max: f64, value: f64) -> Self {
        Self {
            id,
            name: name.into(),
            kind: ControlKind::Slider { min, max },
            value: Some(value.clamp(min, max)),
        }
    }

    pub fn button(id: LayoutId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: ControlKind::Button,
            value: None,
        }
    }

    pub fn label(id: LayoutId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: ControlKind::Label,
            value: None,
        }
    }

    /// **CI-b**：单选组（`selected` 超出范围 ⇒ clamp 到最后一个）。
    pub fn radio_group(
        id: LayoutId,
        name: impl Into<String>,
        options: &'static [&'static str],
        selected: usize,
    ) -> Self {
        let last = options.len().saturating_sub(1);
        Self {
            id,
            name: name.into(),
            kind: ControlKind::RadioGroup { options },
            value: Some(selected.min(last) as f64),
        }
    }
}

/// 一次输入（指针三态；键 / 文本 / 命令留给后续批次）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlEvent {
    Moved(Point),
    Pressed(Point),
    Released(Point),
}

/// 状态机产出的外部意图。
#[derive(Debug, Clone, PartialEq)]
pub enum ControlOutcome {
    ValueChanged { id: LayoutId, value: f64 },
    Activated { id: LayoutId },
}

/// 控件集合 + 交互状态。
#[derive(Debug, Clone)]
pub struct ControlSet {
    specs: Vec<ControlSpec>,
    /// 与 `specs` 同序的落点（布局只在 [`Self::frame`] 时计算）。
    rects: Vec<Rect>,
    router: Router,
    /// 按下时命中的按钮（抬起仍在同一按钮内才激活）。
    pressed_buttons: BTreeSet<LayoutId>,
    /// 行距 / 内边距。
    gap: f64,
    padding: f64,
}

impl ControlSet {
    pub fn new(specs: Vec<ControlSpec>) -> Self {
        let rects = vec![Rect::default(); specs.len()];
        Self {
            specs,
            rects,
            router: Router::new(),
            pressed_buttons: BTreeSet::new(),
            gap: 6.0,
            padding: 4.0,
        }
    }

    pub fn with_spacing(mut self, gap: f64, padding: f64) -> Self {
        self.gap = gap;
        self.padding = padding;
        self
    }

    pub fn specs(&self) -> &[ControlSpec] {
        &self.specs
    }

    pub fn value(&self, id: LayoutId) -> Option<f64> {
        self.specs
            .iter()
            .find(|spec| spec.id == id)
            .and_then(|spec| spec.value)
    }

    /// 当前帧：几何（布局结果）+ 语义（五个锚点）。文本层由宿主填充。
    pub fn frame(&mut self, constraints: Constraints) -> Frame {
        let items: Vec<LayoutItem> = self
            .specs
            .iter()
            .map(|spec| LayoutItem::new(spec.id, constraints.max.width, spec.kind.height()))
            .collect();
        let layout = BoxLayout::new(items, self.gap, self.padding);
        let placed: Vec<PlacedRect> = layout.layout(constraints);
        self.rects = placed.iter().map(|placed| placed.rect).collect();

        let semantics = self
            .specs
            .iter()
            .zip(&self.rects)
            .map(|(spec, rect)| {
                let mut node = SemanticNode::new(role_of(spec.kind), spec.name.clone(), *rect);
                if let Some(value) = spec.value {
                    node = node.with_value(value.to_string());
                }
                node.focusable(spec.kind.interactive())
            })
            .collect();

        Frame {
            geometry: placed,
            // 扁平的控件集合没有容器：全部是根（容器布局由调用方填）。
            parent: vec![None; self.rects.len()],
            semantics,
            text: Vec::new(),
        }
    }

    /// 消费一次输入。命中用**半开矩形**（贴边不双命中），拖拽期间走 capture。
    pub fn handle(&mut self, event: ControlEvent) -> Vec<ControlOutcome> {
        let mut outcomes = Vec::new();
        match event {
            ControlEvent::Moved(point) => {
                let hit = self.index_at(point);
                self.router
                    .pointer_moved(hit.map(|index| route_key(self.specs[index].id)));
                // capture 期间：滑块跟着走（指针移出轨道也不丢抓）。
                if let Some(RouteKey { target, .. }) = self.router.capture().cloned() {
                    let index = target.0 as usize;
                    if let Some(spec) = self.specs.get(index).cloned() {
                        if let ControlKind::Slider { min, max } = spec.kind {
                            let value = value_at(self.rects[index], point, min, max);
                            if self.set_value(index, value) {
                                outcomes.push(ControlOutcome::ValueChanged { id: spec.id, value });
                            }
                        }
                    }
                }
            }
            ControlEvent::Pressed(point) => {
                let hit = self.index_at(point);
                self.router
                    .pressed(hit.map(|index| route_key(self.specs[index].id)));
                self.pressed_buttons.clear();
                let Some(index) = hit else {
                    return outcomes;
                };
                let spec = self.specs[index].clone();
                match spec.kind {
                    ControlKind::Slider { min, max } => {
                        let value = value_at(self.rects[index], point, min, max);
                        if self.set_value(index, value) {
                            outcomes.push(ControlOutcome::ValueChanged { id: spec.id, value });
                        }
                    }
                    ControlKind::Button => {
                        self.pressed_buttons.insert(spec.id);
                    }
                    ControlKind::Label => {}
                    ControlKind::RadioGroup { options } => {
                        let value = radio_index(self.rects[index], point, options.len());
                        if self.set_value(index, value) {
                            outcomes.push(ControlOutcome::ValueChanged { id: spec.id, value });
                        }
                    }
                }
            }
            ControlEvent::Released(point) => {
                if let Some(RouteKey { target, .. }) = self.router.capture().cloned() {
                    let index = target.0 as usize;
                    if let Some(spec) = self.specs.get(index).cloned() {
                        let inside = self
                            .rects
                            .get(index)
                            .is_some_and(|rect| rect.contains(point));
                        if spec.kind == ControlKind::Button
                            && inside
                            && self.pressed_buttons.contains(&spec.id)
                        {
                            outcomes.push(ControlOutcome::Activated { id: spec.id });
                        }
                    }
                }
                self.pressed_buttons.clear();
                self.router.released();
            }
        }
        outcomes
    }

    /// 当前投递目标（capture > focus > hover）的控件 id。
    pub fn routed(&self) -> Option<LayoutId> {
        let key = self.router.dispatch()?;
        self.specs.get(key.target.0 as usize).map(|spec| spec.id)
    }

    /// **CL-R**：结构变化（卸载 / reparent）时显式取消路由。
    ///
    /// 「拖拽中把控件删了」不能停在半截：capture 还指着不存在的目标，指针就会一直
    /// 被当成拖拽。返回被取消的目标 id（诊断用）；没指向任何控件 ⇒ `None`。
    pub fn cancel_routing(&mut self) -> Option<LayoutId> {
        let target = self.routed();
        self.router.cancel();
        target
    }

    /// 卸载一个控件（结构 delta 的消费者）：移除声明并同步落点。
    ///
    /// 返回 `true` 表示确实移除了；`false` = 该 id 不在本集合里（调用方记诊断）。
    pub fn unmount(&mut self, id: LayoutId) -> bool {
        let Some(index) = self.specs.iter().position(|spec| spec.id == id) else {
            return false;
        };
        self.specs.remove(index);
        if index < self.rects.len() {
            self.rects.remove(index);
        }
        self.pressed_buttons.remove(&id);
        // 落点表按声明序，删掉后重排一次（结构变化，不在每帧）。
        self.rects = vec![Rect::default(); self.specs.len()];
        true
    }

    fn set_value(&mut self, index: usize, value: f64) -> bool {
        let Some(spec) = self.specs.get_mut(index) else {
            return false;
        };
        if spec.value == Some(value) {
            return false;
        }
        spec.value = Some(value);
        true
    }

    /// 点 → 控件下标（半开矩形；从后往前找 ⇒ 后声明的在上，与绘制顺序一致）。
    fn index_at(&self, point: Point) -> Option<usize> {
        self.rects.iter().rposition(|rect| rect.contains(point))
    }
}

fn route_key(id: LayoutId) -> RouteKey {
    RouteKey::new("", HitId(id.0))
}

fn role_of(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::Slider { .. } => "slider",
        ControlKind::Button => "button",
        ControlKind::Label => "label",
        ControlKind::RadioGroup { .. } => "radio",
    }
}

/// 单选组的点 → 选项下标（等分轨道；右边界归最后一个）。
fn radio_index(rect: Rect, point: Point, count: usize) -> f64 {
    if count == 0 {
        return 0.0;
    }
    let width = rect.width();
    if width <= 0.0 {
        return 0.0;
    }
    let t = ((point.x - rect.min.x) / width).clamp(0.0, 1.0);
    ((t * count as f64).floor() as usize).min(count - 1) as f64
}

/// 轨道上的 x → 值（线性；超出轨道按端点 clamp）。
fn value_at(rect: Rect, point: Point, min: f64, max: f64) -> f64 {
    let width = rect.width();
    if width <= 0.0 {
        return min;
    }
    let t = ((point.x - rect.min.x) / width).clamp(0.0, 1.0);
    min + t * (max - min)
}

/// 控件集合的尺寸上界（CG-e：约束按声明顺序一次性构造，不每帧重算）。
pub fn constraints_for(specs: &[ControlSpec], max_width: f64) -> Constraints {
    let height: f64 = specs.iter().map(|spec| spec.kind.height()).sum();
    Constraints::new(max_width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> ControlSet {
        ControlSet::new(vec![
            ControlSpec::label(LayoutId(0), "title"),
            ControlSpec::slider(LayoutId(1), "base", 0.0, 1.0, 0.5),
            ControlSpec::button(LayoutId(2), "replay"),
        ])
    }

    fn constraints() -> Constraints {
        Constraints::new(120.0, 400.0)
    }

    /// 滑块的轨道矩形（frame 之后才有）。
    fn slider_rect(set: &mut ControlSet) -> Rect {
        set.frame(constraints()).geometry[1].rect
    }

    #[test]
    fn frame_has_geometry_and_five_anchor_semantics() {
        let mut set = panel();
        let frame = set.frame(constraints());
        assert_eq!(frame.geometry.len(), 3);
        assert_eq!(frame.semantics.len(), 3);
        assert!(frame.text.is_empty(), "文本层由宿主填充");
        let slider = &frame.semantics[1];
        assert_eq!(slider.role, "slider");
        assert_eq!(slider.name, "base");
        assert_eq!(slider.value.as_deref(), Some("0.5"));
        assert!(slider.focusable);
        assert!(!frame.semantics[0].focusable, "标签不可获得焦点");
    }

    #[test]
    fn slider_drag_writes_value_and_keeps_capture() {
        let mut set = panel();
        let rect = slider_rect(&mut set);
        let start = Point::new(rect.min.x + rect.width() * 0.25, rect.center().y);
        let outcomes = set.handle(ControlEvent::Pressed(start));
        assert_eq!(
            outcomes,
            vec![ControlOutcome::ValueChanged {
                id: LayoutId(1),
                value: 0.25
            }]
        );
        // 拖到轨道右端之外：仍然写（capture），并 clamp 到 1.0。
        let outside = Point::new(rect.max.x + 500.0, rect.center().y);
        let outcomes = set.handle(ControlEvent::Moved(outside));
        assert_eq!(
            outcomes,
            vec![ControlOutcome::ValueChanged {
                id: LayoutId(1),
                value: 1.0
            }]
        );
        assert_eq!(set.routed(), Some(LayoutId(1)), "拖拽期间路由锁定滑块");
        set.handle(ControlEvent::Released(outside));
        assert_eq!(set.router.capture(), None, "抬起解除 capture");
        // focus 保留到下一次按下（与 Router 的既有语义一致）；按到空白才清空。
        assert_eq!(set.routed(), Some(LayoutId(1)));
        set.handle(ControlEvent::Pressed(Point::new(-100.0, -100.0)));
        assert_eq!(set.routed(), None, "按到空白 ⇒ 路由清空");
    }

    #[test]
    fn button_activates_only_when_released_inside() {
        let mut set = panel();
        let frame = set.frame(constraints());
        let rect = frame.geometry[2].rect;
        let inside = rect.center();
        let outside = Point::new(rect.max.x + 10.0, rect.center().y);

        assert!(set.handle(ControlEvent::Pressed(inside)).is_empty());
        assert_eq!(
            set.handle(ControlEvent::Released(inside)),
            vec![ControlOutcome::Activated { id: LayoutId(2) }]
        );

        assert!(set.handle(ControlEvent::Pressed(inside)).is_empty());
        assert!(
            set.handle(ControlEvent::Released(outside)).is_empty(),
            "抬起在按钮之外 ⇒ 不激活"
        );
    }

    #[test]
    fn label_never_produces_outcomes() {
        let mut set = panel();
        let rect = set.frame(constraints()).geometry[0].rect;
        let center = rect.center();
        assert!(set.handle(ControlEvent::Pressed(center)).is_empty());
        assert!(set.handle(ControlEvent::Released(center)).is_empty());
    }

    /// **CG-d/c 判据**：同一声明 + 同一事件流 ⇒ 两帧逐位相同。
    #[test]
    fn same_declaration_and_events_produce_the_same_frame() {
        let play = |set: &mut ControlSet| {
            let rect = set.frame(constraints()).geometry[1].rect;
            let mid = Point::new(rect.center().x, rect.center().y);
            set.handle(ControlEvent::Pressed(mid));
            set.handle(ControlEvent::Moved(Point::new(rect.max.x, mid.y)));
            set.handle(ControlEvent::Released(Point::new(rect.max.x, mid.y)));
            set.frame(constraints())
        };
        let mut a = panel();
        let mut b = panel();
        assert_eq!(play(&mut a), play(&mut b));
    }

    #[test]
    fn constraints_are_derived_from_declaration_order() {
        let specs = panel().specs().to_vec();
        let constraints = constraints_for(&specs, 200.0);
        assert_eq!(constraints.max.width, 200.0);
        assert_eq!(constraints.max.height, 16.0 + 20.0 + 24.0);
    }

    /// **CI-b**：单选组的值 = 选项下标；点哪段选哪项，越界 clamp。
    #[test]
    fn radio_group_selects_the_segment_under_the_pointer() {
        static OPTIONS: [&str; 3] = ["2D", "轨道", "第一人称"];
        let mut set = ControlSet::new(vec![ControlSpec::radio_group(
            LayoutId(0),
            "viewport_camera",
            &OPTIONS,
            0,
        )]);
        let frame = set.frame(Constraints::new(300.0, 400.0));
        assert_eq!(frame.semantics[0].role, "radio");
        assert_eq!(frame.semantics[0].value.as_deref(), Some("0"));
        assert!(frame.semantics[0].focusable);
        let rect = frame.geometry[0].rect;

        let at = |t: f64| Point::new(rect.min.x + rect.width() * t, rect.center().y);
        assert_eq!(
            set.handle(ControlEvent::Pressed(at(0.55))),
            vec![ControlOutcome::ValueChanged {
                id: LayoutId(0),
                value: 1.0
            }]
        );
        assert_eq!(
            set.handle(ControlEvent::Pressed(at(0.999))),
            vec![ControlOutcome::ValueChanged {
                id: LayoutId(0),
                value: 2.0
            }],
            "右端归最后一个选项"
        );
        set.handle(ControlEvent::Released(at(0.999)));
        assert_eq!(set.value(LayoutId(0)), Some(2.0));
        assert_eq!(
            set.frame(Constraints::new(300.0, 400.0)).semantics[0]
                .value
                .as_deref(),
            Some("2")
        );
    }
}
