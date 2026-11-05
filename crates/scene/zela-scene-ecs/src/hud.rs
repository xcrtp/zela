//! **CG-f / CL-R**：自研 UI 的第一块可见面板 —— 一行一个**声明实体**，绘制出命令列表。
//!
//! # 它把什么接起来
//!
//! ```text
//! ControlSpec（声明）  →  ControlSet（ze-interact 状态机，命中 / capture）
//!        ↓ 装配期一次
//! WidgetDecl（每行一个实体，进产物）
//!        ↓ ui_layout：声明树 → 编译 → 布局 → Frame
//! WidgetRuntime::draw → Vec<DrawCommand> → 渲染层（相机仿射后入队）
//! ```
//!
//! # CL-R 把什么删了
//!
//! 旧版每行 spawn「轨道 `Rect` + N 个字形 `Path` + 滑块 `Circle`」：20 行就是几百个实体，
//! 而且滑块圆心的更新要反查 `ObjectId`。现在每行只有**一个** `WidgetDecl` 实体，
//! 轨道 / 字形 / 滑块都是同一份声明 `draw` 出来的命令；滑块位置由 `value` 决定，
//! 不再有需要逐帧搬的圆实体。
//!
//! # 布局不在每帧
//!
//! 摆放只在**装配期**做一次（[`build`]）；运行时只改 `ControlSet` 的值并重新出命令。
//! 布局结果同时是命中矩形：`ui_layout` 的前推分配与 `ControlSet` 的 `BoxLayout`
//! 逐位相同（用例 `frame_matches_the_state_machine_geometry` 守着这条）。

use ze_interact::widget::{
    BuiltinWidget, DrawCtx, DrawEnv, GlyphOutline, HitPart, Intent, IntentCtx, LayoutTokens,
    Metrics, StrategyId, Theme, ThemeId, ThemeTable, WidgetDecl, WidgetPhase, WidgetRuntime,
    WidgetSpecId,
};
use ze_interact::{
    ControlEvent, ControlKind, ControlOutcome, ControlSet, ControlSpec, Frame, LayoutId, Point,
};
use ze_notion::{Path2D, Rect2D, Transform2D, Vec2};
use ze_render_ir::{DrawCommand, ZOrder};
use ze_scene_contract::Scene as _;
use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

use crate::Scene;
use crate::domain::{InputEvents, SceneEvent};
use crate::ui_layout::{LayoutResult, layout_widgets};

/// 面板各层的绘制层级（轨道 → 标签 +10 → 滑块 +20，与 `BuiltinWidget::draw` 同口径）。
pub const HUD_TRACK_Z: i32 = 100;
pub const HUD_LABEL_Z: i32 = 110;
pub const HUD_KNOB_Z: i32 = 120;

/// 世界坐标口径：面板原点 / 宽度 / 行高 / 单位换算。
#[derive(Debug, Clone, Copy)]
pub struct HudMetrics {
    /// 面板左上角（世界坐标）。
    pub origin: Vec2,
    /// 面板宽度（世界单位）。
    pub width: f32,
    /// 一行的高度（世界单位）。
    pub row_height: f32,
    /// 行距（世界单位）。
    pub gap: f32,
    /// 内边距（世界单位）。
    pub padding: f32,
    /// 标签字号（世界单位）。
    pub label_font_size: f32,
    /// 滑块半径（世界单位）。
    pub knob_radius: f32,
}

impl HudMetrics {
    /// 默认口径：与 CE-e 的面板同一量级（宽 6、行高 0.55）。
    pub fn default_metrics() -> Self {
        Self {
            origin: Vec2::ZERO,
            width: 6.0,
            row_height: 0.55,
            gap: 0.20,
            padding: 0.25,
            label_font_size: 0.30,
            knob_radius: 0.16,
        }
    }

    /// 一个布局单位对应多少世界单位（控件的自然高度是 20 布局单位）。
    fn unit(&self) -> f32 {
        self.row_height / 20.0
    }

    /// 世界坐标 → 布局坐标（控件局部的 `[0, width] × [0, height]`）。
    pub fn to_layout(&self, world: Vec2) -> Point {
        let unit = self.unit();
        Point::new(
            f64::from((world.x - self.origin.x) / unit),
            f64::from((world.y - self.origin.y) / unit),
        )
    }

    fn to_world(&self, rect: ze_interact::Rect) -> Rect2D {
        let unit = self.unit();
        Rect2D::new(
            Vec2::new(
                self.origin.x + rect.min.x as f32 * unit,
                self.origin.y + rect.min.y as f32 * unit,
            ),
            Vec2::new(
                self.origin.x + rect.max.x as f32 * unit,
                self.origin.y + rect.max.y as f32 * unit,
            ),
        )
    }

    fn inverse_transform(&self) -> Transform2D {
        let unit = self.unit();
        Transform2D::scale(Vec2::splat(1.0 / unit)).compose(Transform2D::translate(-self.origin))
    }

    fn draw_metrics(&self) -> Metrics {
        Metrics::new(
            f64::from(self.unit()),
            Point::new(f64::from(self.origin.x), f64::from(self.origin.y)),
        )
    }
}

/// 一行控件（装配期算一次）。
#[derive(Debug, Clone)]
struct Row {
    id: LayoutId,
    /// 布局单位下的控件矩形（与 `Frame.geometry` 同源）。
    rect: ze_interact::Rect,
    /// 应用级意图：**装配期解析一次**，事件路径不再按名字 parse。
    value_intent: Option<ValueIntent>,
    action_intent: Option<ActionIntent>,
}

/// 场景里的一块自研面板。
#[derive(Debug, Clone)]
pub struct Hud {
    set: ControlSet,
    metrics: HudMetrics,
    decls: Vec<WidgetDecl>,
    rows: Vec<Row>,
    runtime: WidgetRuntime,
    layout: LayoutResult,
    /// 重排用的输入（结构变化时按同一份声明重算一次）。
    root: ze_interact::Rect,
    layout_tokens: LayoutTokens,
    themes: ThemeTable,
    theme_id: ThemeId,
    env: DrawEnv,
    /// 本帧状态机产出的外部意图（**镜像**，供 debug 回执读取）。
    last_outcomes: Vec<HudOutcome>,
    /// **CK-e**：本帧的回写诊断（未知控件名等）。空 = 没有异常。
    last_errors: Vec<String>,
}

/// HUD 一次交互的外部意图（镜像 `ControlOutcome`，**不派生 serde**）。
#[derive(Debug, Clone, PartialEq)]
pub struct HudOutcome {
    /// 控件声明下标（与 `Frame::geometry` 同一口径）。
    pub control: LayoutId,
    /// 语义名（诊断可读）。
    pub name: String,
    pub kind: HudOutcomeKind,
}

/// 意图类型。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HudOutcomeKind {
    ValueChanged(f64),
    Activated,
}

/// 数值回写的 typed intent（**装配期**由控件名解析一次；事件路径按枚举走）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueIntent {
    /// 参数地址（`panel_a.base` 这类带 `:` 的注册表键）。
    Parameter(String),
    Speed,
    Seek,
    Show2d,
    Show3d,
    ViewportCamera,
    Free2dCamera,
    FlipLayerOrder,
}

impl ValueIntent {
    /// 装配期解析；未知 ⇒ `None`（装配时记诊断，不静默吞）。
    pub fn of(name: &str) -> Option<Self> {
        if name.contains(':') {
            return Some(Self::Parameter(name.to_string()));
        }
        Some(match name {
            "speed" => Self::Speed,
            "seek" => Self::Seek,
            "show_2d" => Self::Show2d,
            "show_3d" => Self::Show3d,
            "viewport_camera" => Self::ViewportCamera,
            "free_2d_camera" => Self::Free2dCamera,
            "flip_layer_order" => Self::FlipLayerOrder,
            _ => return None,
        })
    }
}

/// 激活类回写的 typed intent。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionIntent {
    PlayPause,
    Replay,
    ShowGrid,
    ShowField,
    FilterGain,
    FilterDistortion,
    FilterBlur,
}

impl ActionIntent {
    pub fn of(name: &str) -> Option<Self> {
        Some(match name {
            "play_pause" => Self::PlayPause,
            "replay" => Self::Replay,
            "show_grid" => Self::ShowGrid,
            "show_field" => Self::ShowField,
            "filter_gain" => Self::FilterGain,
            "filter_distortion" => Self::FilterDistortion,
            "filter_blur" => Self::FilterBlur,
            _ => return None,
        })
    }
}

fn spec_of(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::Slider { .. } => "slider",
        ControlKind::Button => "button",
        ControlKind::Label => "label",
        ControlKind::RadioGroup { .. } => "radio",
    }
}

fn role_of(kind: ControlKind) -> &'static str {
    spec_of(kind)
}

impl Hud {
    /// 当前帧（几何 + 父索引 + 语义 + 文本）。
    ///
    /// 几何 / 父索引来自装配期布局（**不在每帧重排**）；语义与文本按当前 `runtime`
    /// 的值刷新 —— 拖动滑块后 `Frame.semantics[].value` 必须当帧可见。
    pub fn frame(&self) -> Frame {
        let mut frame = self.layout.frame.clone();
        for (index, placed) in frame.geometry.iter().enumerate() {
            let Some(widget) = self.runtime.get(placed.id) else {
                continue;
            };
            let state = widget.state();
            let mut node =
                ze_interact::SemanticNode::new(state.role.clone(), state.name.clone(), placed.rect);
            if let Some(value) = state.value {
                node = node.with_value(value);
            }
            frame.semantics[index] = node.focusable(state.focusable);
            frame.text[index] = widget.text_run().unwrap_or_default();
        }
        frame
    }

    /// 本帧每个控件的布局矩形（布局单位；`Frame::geometry` 的唯一来源）。
    pub fn bounds(&self) -> &std::collections::BTreeMap<LayoutId, ze_interact::Rect> {
        &self.layout.bounds
    }

    pub fn set(&self) -> &ControlSet {
        &self.set
    }

    pub fn value(&self, id: LayoutId) -> Option<f64> {
        self.set.value(id)
    }

    /// 世界坐标 → 控件事件。落在面板外 ⇒ 仍然传（状态机会忽略）。
    ///
    /// **CL-R**：先用 `HitChain::visit` 沿命中链问「谁消费这次指针」，链上没有控件
    /// 消费（点在外面）且当前也没有 capture / focus ⇒ 直接不转发。
    /// 命中链的消费规则是控件自己的 `on_intent(&Intent::Pointer, ..)`。
    pub fn handle_world(&mut self, event: SceneEvent) -> Vec<ControlOutcome> {
        let point = |pos: Vec2| self.metrics.to_layout(pos);
        match event {
            SceneEvent::ViewportHover { scene_pos, .. } => {
                let at = point(scene_pos);
                if !self.pointer_is_consumed(at) && self.set.routed().is_none() {
                    return Vec::new();
                }
                self.set.handle(ControlEvent::Moved(at))
            }
            SceneEvent::ViewportClick { scene_pos, .. } => {
                let at = point(scene_pos);
                if !self.pointer_is_consumed(at) && self.set.routed().is_none() {
                    return Vec::new();
                }
                self.set.handle(ControlEvent::Pressed(at))
            }
            SceneEvent::ViewportRelease { scene_pos, .. } => {
                // 松手永远转发：capture 必须有机会被清掉。
                self.set.handle(ControlEvent::Released(point(scene_pos)))
            }
            _ => Vec::new(),
        }
    }

    /// 某点的命中链（按 id 降序；行之间不重叠 ⇒ 顺序只影响诊断可读性）。
    pub fn hit_chain_at(&self, point: Point) -> ze_interact::HitChain {
        let mut hits: Vec<ze_interact::HitId> = self
            .layout
            .bounds
            .iter()
            .filter(|(_, rect)| rect.contains(point))
            .map(|(&id, _)| ze_interact::HitId(id.0))
            .collect();
        hits.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        ze_interact::HitChain::new(hits)
    }

    /// 沿命中链问「谁消费指针」——`HitChain::visit` 是本项目唯一的冒泡实现。
    fn pointer_is_consumed(&mut self, point: Point) -> bool {
        let chain = self.hit_chain_at(point);
        let metrics = self.metrics.draw_metrics();
        let bounds = self.layout.bounds.clone();
        let mut runtime = std::mem::take(&mut self.runtime);
        let consumed = chain
            .visit(|hit| {
                let id = LayoutId(hit.0);
                let rect = bounds.get(&id).copied().unwrap_or_default();
                let ctx = IntentCtx {
                    part: HitPart::Body,
                    bounds: rect,
                    metrics: &metrics,
                    phase: WidgetPhase::FrameEnd,
                };
                match runtime.get_mut(id) {
                    Some(widget) => match widget.on_intent(&Intent::Pointer(point), &ctx) {
                        ze_interact::widget::Verdict::Consume => ze_interact::HitFlow::Consume,
                        ze_interact::widget::Verdict::Continue => ze_interact::HitFlow::Continue,
                    },
                    None => ze_interact::HitFlow::Continue,
                }
            })
            .is_some();
        self.runtime = runtime;
        consumed
    }

    /// **结构 delta 的消费者**：挂载 / 卸载 / reparent。
    ///
    /// - 卸载：删声明 + 删行 + `ControlSet::unmount`；若 capture / focus 正指着它，
    ///   **显式取消并记诊断**（不静默）；
    /// - reparent：改声明的 `parent`，重排一次；
    /// - 挂载：`StructureDelta` 只带 id / parent（不带 spec / label），没有声明的挂载
    ///   只能显式记账 —— 不伪造一个空控件。
    ///
    /// 返回 `true` 表示声明树变了、已重排。
    pub fn apply_structure_delta(&mut self, ops: &[crate::domain::StructureOp]) -> bool {
        use crate::domain::StructureOp;
        let mut changed = false;
        for op in ops {
            match op {
                StructureOp::Unmount { id } => {
                    let id = LayoutId(id.0);
                    if !self.decls.iter().any(|decl| decl.id == id) {
                        self.record_error(format!("卸载未知控件 `{}`（声明里没有）", id.0));
                        continue;
                    }
                    let cancelled = if self.set.routed() == Some(id) {
                        self.set.cancel_routing()
                    } else {
                        None
                    };
                    self.decls.retain(|decl| decl.id != id);
                    self.rows.retain(|row| row.id != id);
                    self.set.unmount(id);
                    if cancelled.is_some() {
                        self.record_error(format!(
                            "控件 {} 在拖拽 / 聚焦中被卸载：capture 已显式取消",
                            id.0
                        ));
                    }
                    changed = true;
                }
                StructureOp::Reparent { id, parent } => {
                    let id = LayoutId(id.0);
                    let parent = parent.map(|parent| LayoutId(parent.0));
                    let Some(decl) = self.decls.iter_mut().find(|decl| decl.id == id) else {
                        self.record_error(format!("reparent 未知控件 `{}`", id.0));
                        continue;
                    };
                    decl.parent = parent;
                    changed = true;
                }
                StructureOp::Mount { id, .. } => {
                    let id = LayoutId(id.0);
                    if self.decls.iter().any(|decl| decl.id == id) {
                        continue;
                    }
                    self.record_error(format!(
                        "挂载 `{}` 缺少声明（StructureDelta 不带 spec / label），未创建空控件",
                        id.0
                    ));
                }
            }
        }
        if changed {
            self.relayout();
        }
        changed
    }

    /// 结构变化后重排一次（布局不在每帧）。
    fn relayout(&mut self) {
        self.decls.sort_by_key(|decl| decl.id);
        let unit = self.metrics.unit().max(f32::EPSILON);
        self.set.frame(ze_interact::Constraints::new(
            f64::from(self.metrics.width / unit),
            f64::INFINITY,
        ));
        match layout_widgets(
            &self.decls,
            &mut self.runtime,
            &mut ze_interact::solver::LayoutEngine::new(),
            self.root,
            &self.layout_tokens,
            &self.env,
        ) {
            Ok(layout) => {
                for row in &mut self.rows {
                    if let Some(rect) = layout.bounds.get(&row.id) {
                        row.rect = *rect;
                    }
                }
                self.layout = layout;
            }
            Err(error) => self.record_error(format!("结构变化后重排失败：{error}")),
        }
    }

    /// 状态机 → 控件自身：typed intent 由控件的 `on_intent` 消费（写回自己的 props）。
    pub fn sync_from_set(&mut self) {
        for row in &self.rows {
            if let Some(value) = self.set.value(row.id) {
                self.runtime.apply_value(row.id, value);
            }
        }
    }

    /// 按语义名找控件（Agent / 快捷键用）。
    pub fn id_by_name(&self, name: &str) -> Option<LayoutId> {
        self.set
            .specs()
            .iter()
            .find(|spec| spec.name == name)
            .map(|spec| spec.id)
    }

    /// 控件在**世界坐标**里的矩形 —— Agent 把"点哪儿"换算成场景坐标的唯一入口。
    pub fn world_rect(&self, id: LayoutId) -> Option<Rect2D> {
        self.layout
            .bounds
            .get(&id)
            .map(|rect| self.metrics.to_world(*rect))
    }

    /// 装配期解析好的应用级数值意图（事件路径不再 parse 名字）。
    pub fn value_intent(&self, id: LayoutId) -> Option<&ValueIntent> {
        self.rows
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| row.value_intent.as_ref())
    }

    /// 装配期解析好的应用级激活意图。
    pub fn action_intent(&self, id: LayoutId) -> Option<ActionIntent> {
        self.rows
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| row.action_intent)
    }

    /// 当前投递目标（capture > focus > hover）的**语义名**：诊断里「命中谁」。
    pub fn routed_name(&self) -> Option<&str> {
        let id = self.set.routed()?;
        self.set
            .specs()
            .iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.name.as_str())
    }

    /// 本帧状态机产出的意图（debug 回执读取；空 = 本帧没命中任何控件）。
    pub fn outcomes(&self) -> &[HudOutcome] {
        &self.last_outcomes
    }

    /// 清空意图（每帧开头调用；状态机本身只返回 `Vec`）。
    pub fn clear_outcomes(&mut self) {
        self.last_outcomes.clear();
        self.last_errors.clear();
    }

    /// 本帧回写诊断（未知控件名 / 参数写入被拒）。
    pub fn errors(&self) -> &[String] {
        &self.last_errors
    }

    /// 诊断回执用的单行说明（空 = `None`）。
    pub fn error_note(&self) -> Option<String> {
        if self.last_errors.is_empty() {
            None
        } else {
            Some(self.last_errors.join("；"))
        }
    }

    /// 本面板的主题 id（进 `Diagnostics.environment`）。
    pub fn theme(&self) -> &str {
        &self.theme_id.0
    }

    /// **本帧命令列表**（世界坐标；`Shape2D` 变体零改动）。
    ///
    /// 20 行 = 20 个声明实体，命令条数由「轨道 + 字形 + 滑块」决定，**没有**逐字形实体。
    pub fn commands(&self) -> Vec<DrawCommand> {
        let metrics = self.metrics.draw_metrics();
        let ctx = DrawCtx {
            bounds: ze_interact::Rect::default(),
            clip: None,
            theme_id: &self.theme_id,
            themes: &self.themes,
            metrics: &metrics,
            env: &self.env,
            z_base: ZOrder(HUD_TRACK_Z),
            hover: false,
            phase: WidgetPhase::FrameEnd,
        };
        let hover = self.set.routed();
        self.runtime.draw_all(&self.layout.bounds, &ctx, hover)
    }

    /// 控件声明（进产物的那一份；调试 / 回执用）。
    pub fn decls(&self) -> &[WidgetDecl] {
        &self.decls
    }

    pub fn record_error(&mut self, message: String) {
        self.last_errors.push(message);
    }
}

/// **装配期**：把一组控件摆成场景**声明**（一行一个实体），返回可交互的 [`Hud`]。
///
/// `bake_label(text, anchor_world, font_size)` 由调用方实现（直排文字引擎 / typst）：
/// 返回若干条**世界坐标**轮廓（一个字形一条）；本函数负责折回布局单位。
pub fn build(
    scene: &mut Scene,
    specs: Vec<ControlSpec>,
    metrics: HudMetrics,
    mut bake_label: impl FnMut(&str, Vec2, f32) -> Vec<Path2D>,
) -> Hud {
    let unit = metrics.unit().max(f32::EPSILON);
    let gap_units = f64::from(metrics.gap / unit);
    let padding_units = f64::from(metrics.padding / unit);
    let mut set = ControlSet::new(specs.clone()).with_spacing(gap_units, padding_units);
    // 命中矩形来自状态机自己的 `BoxLayout`；布局链（`ui_layout`）与它逐位相同
    // （用例 `frame_matches_the_state_machine_geometry`），两边各自只算一次。
    set.frame(ze_interact::Constraints::new(
        f64::from(metrics.width / unit),
        f64::INFINITY,
    ));

    let decls: Vec<WidgetDecl> = specs
        .iter()
        .map(|spec| {
            let (value_min, value_max) = match spec.kind {
                ControlKind::Slider { min, max } => (min, max),
                ControlKind::RadioGroup { options } => {
                    (0.0, options.len().saturating_sub(1) as f64)
                }
                _ => (0.0, 0.0),
            };
            WidgetDecl {
                id: spec.id,
                spec: WidgetSpecId::new(spec_of(spec.kind)),
                parent: None,
                label: spec.name.clone(),
                role: role_of(spec.kind).to_string(),
                value: spec.value.unwrap_or(0.0),
                value_min,
                value_max,
                strategy: StrategyId::default(),
                constraints: Default::default(),
            }
        })
        .collect();

    let layout_tokens = LayoutTokens {
        row_height: 20.0,
        gap: gap_units,
        padding: padding_units,
        font_size: f64::from(metrics.label_font_size / unit),
        knob_radius: f64::from(metrics.knob_radius / unit),
    };
    let env = DrawEnv::default();
    let width_units = f64::from(metrics.width / unit);
    let root = ze_interact::Rect::from_min_size(Point::new(0.0, 0.0), width_units, 0.0);

    let mut runtime = runtime_with_factories();
    let mut layout = layout_widgets(
        &decls,
        &mut runtime,
        &mut ze_interact::solver::LayoutEngine::new(),
        root,
        &layout_tokens,
        &env,
    )
    .expect("HUD 声明必须能布局");

    // 标签：按世界坐标烘焙，再折回布局单位（`BuiltinWidget::draw` 统一映射回世界）。
    let inverse = metrics.inverse_transform();
    let text_height = metrics.label_font_size * 1.3;
    for placed in layout.frame.geometry.clone() {
        let world = metrics.to_world(placed.rect);
        let label_y = world.max.y + ((metrics.gap - text_height) * 0.5).max(0.0);
        let name = decls
            .iter()
            .find(|decl| decl.id == placed.id)
            .map(|decl| decl.label.clone())
            .unwrap_or_default();
        let glyphs = bake_label(
            &name,
            Vec2::new(world.min.x, label_y),
            metrics.label_font_size,
        );
        let glyphs: Vec<GlyphOutline> = glyphs
            .into_iter()
            .map(|path| GlyphOutline {
                path: path.transformed(&inverse),
            })
            .collect();
        runtime.set_glyphs(placed.id, glyphs);
    }
    // 字形已注入 ⇒ 再重建一次 dyn 表（装配期一次，不在每帧）。
    layout = layout_widgets(
        &decls,
        &mut runtime,
        &mut ze_interact::solver::LayoutEngine::new(),
        root,
        &layout_tokens,
        &env,
    )
    .expect("HUD 声明必须能布局");

    // **一行一个实体**：声明进产物（几何 / 字形都不进）。
    for decl in &decls {
        scene
            .spawn_dyn(vec![Box::new(decl.clone())])
            .expect("HUD：WidgetDecl 已登记");
    }

    let rows = decls
        .iter()
        .zip(specs.iter())
        .map(|(decl, spec)| Row {
            id: decl.id,
            rect: layout.bounds.get(&decl.id).copied().unwrap_or_default(),
            value_intent: ValueIntent::of(&spec.name),
            action_intent: ActionIntent::of(&spec.name),
        })
        .collect();

    let theme_id = Theme::default_theme().id;
    Hud {
        set,
        metrics,
        decls,
        rows,
        runtime,
        layout,
        root,
        layout_tokens,
        themes: ThemeTable::with_default(),
        theme_id,
        env,
        last_outcomes: Vec::new(),
        last_errors: Vec::new(),
    }
}

/// 内置控件的规格表（`SpecId → 工厂`；第三方只加表项，不改 `Shape2D`）。
pub fn runtime_with_factories() -> WidgetRuntime {
    let mut runtime = WidgetRuntime::new();
    for spec in ["slider", "button", "label", "radio"] {
        runtime.register(spec, |props| Box::new(BuiltinWidget::new(props.clone())));
    }
    // **自定义规格样例**：第三方控件只注册一个表项，不碰 `Shape2D` / 后端。
    runtime.register(SPARKLINE_SPEC, |props| {
        Box::new(SparklineWidget::new(props.clone()))
    });
    runtime
}

/// 自定义控件的规格名（R-20：它进结构摘要，换规格 ⇒ 摘要变）。
pub const SPARKLINE_SPEC: &str = "zela.sparkline";

/// **第三方控件样例**：一条折线（`Shape2D::Polyline`）。
///
/// 它证明「自定义控件 = 注册一个 `SpecId` 工厂」，不需要改 `Shape2D` 变体、
/// 也不需要改渲染后端。声明进产物；绘制仍是纯函数。
#[derive(Debug)]
pub struct SparklineWidget {
    props: ze_interact::widget::WidgetProps,
    bounds: ze_interact::Rect,
}

impl SparklineWidget {
    pub fn new(props: ze_interact::widget::WidgetProps) -> Self {
        Self {
            props,
            bounds: ze_interact::Rect::default(),
        }
    }

    fn samples(&self) -> Vec<ze_notion::Vec2> {
        let count = 16usize;
        (0..count)
            .map(|index| {
                let t = index as f32 / (count - 1) as f32;
                let phase = t * std::f32::consts::TAU;
                let value = (phase * 2.0).sin() * 0.5 + 0.5;
                ze_notion::Vec2::new(
                    self.bounds.min.x as f32 + t * self.bounds.width() as f32,
                    self.bounds.max.y as f32 - value * self.bounds.height() as f32,
                )
            })
            .collect()
    }
}

impl ze_interact::widget::Widget2D for SparklineWidget {
    fn measure(&self, ctx: &ze_interact::widget::MeasureCtx<'_>) -> ze_interact::Size {
        ze_interact::Size::new(ctx.constraints.max.width, 20.0)
    }

    fn set_bounds(&mut self, bounds: ze_interact::Rect) {
        self.bounds = bounds;
    }

    fn bounds(&self) -> ze_interact::Rect {
        self.bounds
    }

    fn state(&self) -> ze_interact::widget::WidgetState {
        ze_interact::widget::WidgetState {
            role: "sparkline".to_string(),
            name: self.props.label.clone(),
            value: None,
            focusable: false,
        }
    }

    fn draw(&self, ctx: &DrawCtx<'_>) -> Vec<DrawCommand> {
        let theme = ctx.theme();
        let origin = ctx.metrics.origin;
        let scale = ctx.metrics.scale as f32;
        let points = self
            .samples()
            .into_iter()
            .map(|point| {
                ze_notion::Vec2::new(
                    origin.x as f32 + point.x * scale,
                    origin.y as f32 + point.y * scale,
                )
            })
            .collect();
        vec![DrawCommand::new(
            ctx.z_base,
            ze_render_ir::Shape2D::Polyline { points },
            ze_render_ir::Style2D {
                stroke: Some(ze_notion::Color32::new(
                    theme.paint.label[0],
                    theme.paint.label[1],
                    theme.paint.label[2],
                    theme.paint.label[3],
                )),
                stroke_width: scale,
                ..ze_render_ir::Style2D::default()
            },
        )]
    }
}

/// HUD 资源（宿主注入；`None` = 本场景没有自研面板）。
#[derive(Debug, Clone, Default)]
pub struct HudState(pub Option<Hud>);

impl HudState {
    pub fn new(hud: Hud) -> Self {
        Self(Some(hud))
    }

    pub fn hud(&self) -> Option<&Hud> {
        self.0.as_ref()
    }

    pub fn hud_mut(&mut self) -> Option<&mut Hud> {
        self.0.as_mut()
    }
}

/// **HUD System**：消费视口事件 → 控件状态机 → 写回场景资源 + 出本帧命令。
///
/// 复用 CE-e 的回写入口（`params::apply` / `DirectorState` / `UiConfig`），
/// 不新建第二套参数寻址。
pub struct HudSystem;

impl System for HudSystem {
    type Reads = (InputEvents, HudState);
    type Writes = (
        HudState,
        crate::DirectorState,
        crate::domain::UiConfig,
        crate::animation::BindingTable,
        ze_scene_core::WidgetCommands,
    );

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        let events: Vec<SceneEvent> = match ctx.get::<InputEvents>() {
            Some(events) => events.iter().cloned().collect(),
            None => return,
        };
        if events.is_empty() {
            // 没有输入也要清掉上一帧的意图镜像：否则 debug 回执会把旧 outcome
            // 当成这一帧的。
            if let Some(hud) = ctx.get_mut::<HudState>().and_then(HudState::hud_mut) {
                if !hud.outcomes().is_empty() {
                    hud.clear_outcomes();
                }
            }
            // 命令仍要发布：渲染层每帧都要读到本帧的命令列表。
            Self::publish(ctx);
            return;
        }
        let Some(mut hud) = ctx
            .get_mut::<HudState>()
            .and_then(HudState::hud_mut)
            .cloned()
        else {
            return;
        };

        let mut outcomes = Vec::new();
        hud.clear_outcomes();
        // **CK-d / CL-R**：结构 delta 在**帧边界**生效 —— 先应用，再分发指针事件
        // （否则冒泡正沿命中链上溯时树变了）。
        let mut structure_changed = false;
        for event in &events {
            if let SceneEvent::StructureDelta { ops } = event {
                structure_changed |= hud.apply_structure_delta(ops);
            }
        }
        if structure_changed {
            hud.sync_from_set();
        }
        for event in &events {
            if matches!(event, SceneEvent::StructureDelta { .. }) {
                continue;
            }
            outcomes.extend(hud.handle_world(event.clone()));
        }
        // **不能**按 `outcomes.is_empty()` 提前返回：`Released` / `Hover` 只改**路由状态**
        // （capture / focus），不产生 outcome。
        if !outcomes.is_empty() {
            let mut director = ctx
                .get::<crate::DirectorState>()
                .cloned()
                .unwrap_or_default();
            let mut ui = ctx
                .get::<crate::domain::UiConfig>()
                .cloned()
                .unwrap_or_default();
            let mut table = ctx
                .get::<crate::animation::BindingTable>()
                .cloned()
                .unwrap_or_default();

            for outcome in outcomes {
                let (id, kind) = match outcome {
                    ControlOutcome::ValueChanged { id, value } => {
                        (id, HudOutcomeKind::ValueChanged(value))
                    }
                    ControlOutcome::Activated { id } => (id, HudOutcomeKind::Activated),
                };
                match kind {
                    HudOutcomeKind::ValueChanged(value) => {
                        let Some(intent) = hud.value_intent(id).cloned() else {
                            hud.record_error(format!("未知数值控件 `{}`", id.0));
                            continue;
                        };
                        match intent {
                            ValueIntent::Parameter(address) => {
                                // 参数地址（CA 的注册表口径）：与 CE-e 同一条回写路径。
                                if let Err(error) = crate::animation::params::apply(
                                    &mut table,
                                    &address,
                                    value as f32,
                                ) {
                                    hud.record_error(format!(
                                        "参数 `{address}` 写入被拒绝：{error}"
                                    ));
                                }
                            }
                            ValueIntent::Speed => director.director.set_speed(value),
                            ValueIntent::Seek => director.director.seek(value),
                            // **CI-b / CG-08**：图层与相机开关（单选组，值 = 选项下标）。
                            ValueIntent::Show2d => ui.show_2d = value < 0.5,
                            ValueIntent::Show3d => ui.show_3d = value < 0.5,
                            ValueIntent::ViewportCamera => {
                                ui.viewport_camera = match value.round() as i32 {
                                    1 => crate::domain::ViewportCamera::ThreeDOrbit,
                                    2 => crate::domain::ViewportCamera::ThreeDFps,
                                    _ => crate::domain::ViewportCamera::TwoD,
                                }
                            }
                            ValueIntent::Free2dCamera => ui.free_2d_camera = value >= 0.5,
                            ValueIntent::FlipLayerOrder => ui.flip_layer_order = value >= 0.5,
                        }
                    }
                    HudOutcomeKind::Activated => {
                        let Some(intent) = hud.action_intent(id) else {
                            hud.record_error(format!("未知按钮 `{}`", id.0));
                            continue;
                        };
                        match intent {
                            ActionIntent::PlayPause => director.director.toggle(),
                            ActionIntent::Replay => {
                                director.director.seek(0.0);
                                director.director.set_speed(1.0);
                            }
                            ActionIntent::ShowGrid => ui.show_grid = !ui.show_grid,
                            ActionIntent::ShowField => ui.show_field = !ui.show_field,
                            ActionIntent::FilterGain => ui.filter_gain = !ui.filter_gain,
                            ActionIntent::FilterDistortion => {
                                ui.filter_distortion = !ui.filter_distortion
                            }
                            ActionIntent::FilterBlur => ui.filter_blur = !ui.filter_blur,
                        }
                    }
                }
                // 回执镜像：状态机的返回值原样落一份（零 System 传播 tag）。
                hud.record(id, kind);
            }

            // 写回资源。
            if let Some(slot) = ctx.get_mut::<crate::DirectorState>() {
                *slot = director;
            }
            if let Some(slot) = ctx.get_mut::<crate::domain::UiConfig>() {
                *slot = ui;
            }
            if let Some(slot) = ctx.get_mut::<crate::animation::BindingTable>() {
                *slot = table;
            }
        }

        // 状态机 → 控件 props（绘制用的值）→ 本帧命令（渲染层消费）。
        hud.sync_from_set();
        if let Some(slot) = ctx.get_mut::<HudState>() {
            slot.0 = Some(hud);
        }
        Self::publish(ctx);
    }
}

impl HudSystem {
    /// 把当前面板的命令列表发布到资源（渲染层读；无面板 ⇒ 不写）。
    fn publish(ctx: &mut ExecutorContext<'_>) {
        let commands = ctx.get::<HudState>().and_then(HudState::hud).map(|hud| {
            ze_scene_core::WidgetCommands {
                theme: hud.theme().to_string(),
                commands: hud.commands(),
            }
        });
        if let (Some(commands), Some(slot)) =
            (commands, ctx.get_mut::<ze_scene_core::WidgetCommands>())
        {
            *slot = commands;
        }
    }
}

impl Hud {
    fn record(&mut self, control: LayoutId, kind: HudOutcomeKind) {
        let name = self
            .set
            .specs()
            .iter()
            .find(|spec| spec.id == control)
            .map(|spec| spec.name.clone())
            .unwrap_or_default();
        self.last_outcomes.push(HudOutcome {
            control,
            name,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{InputEvents, LayerId};
    use crate::{DirectorState, Scene};
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};

    fn run_frame(scheduler: &mut Scheduler, events: Vec<SceneEvent>) {
        scheduler
            .resources_mut()
            .insert(InputEvents::from_events(events));
        scheduler.run_frame(0.0);
    }

    fn value_of(scheduler: &Scheduler, id: LayoutId) -> f64 {
        scheduler
            .resources()
            .get::<HudState>()
            .and_then(HudState::hud)
            .and_then(|hud| hud.value(id))
            .expect("有滑块值")
    }

    /// **判据 2**：一行一个声明实体；命令仍是「轨道 + 字形 + 滑块」。
    #[test]
    fn one_entity_per_row_and_commands_carry_their_own_style() {
        let mut scene = Scene::new();
        let specs = vec![
            ControlSpec::slider(LayoutId(0), "speed", 0.0, 2.0, 1.0),
            ControlSpec::button(LayoutId(1), "play_pause"),
        ];
        let hud = build(
            &mut scene,
            specs,
            HudMetrics::default_metrics(),
            |_, _, _| Vec::new(),
        );
        assert_eq!(scene.len(), 2, "两行 = 两个实体");
        let commands = hud.commands();
        assert_eq!(commands.len(), 3, "轨道 + 滑块 + 按钮轨道");
        assert_eq!(commands[0].shape.kind_name(), "rect");
        assert_eq!(commands[1].shape.kind_name(), "circle");
        assert_eq!(commands[2].shape.kind_name(), "rect");
        assert!(!hud.theme().is_empty(), "主题 id 必须可回执");
    }

    /// **判据 3**：生产布局填出的几何与状态机的命中矩形逐位相同。
    #[test]
    fn frame_matches_the_state_machine_geometry() {
        let mut scene = Scene::new();
        let metrics = HudMetrics::default_metrics();
        let specs: Vec<ControlSpec> = (0..6)
            .map(|index| match index % 3 {
                0 => ControlSpec::button(LayoutId(index), format!("b{index}")),
                1 => ControlSpec::slider(LayoutId(index), format!("s{index}"), 0.0, 1.0, 0.5),
                _ => ControlSpec::label(LayoutId(index), format!("l{index}")),
            })
            .collect();
        let hud = build(&mut scene, specs, metrics, |_, _, _| Vec::new());
        let frame = hud.frame();
        let mut set = hud.set().clone();
        let unit = metrics.unit();
        let state_frame = set.frame(ze_interact::Constraints::new(
            f64::from(metrics.width / unit),
            f64::INFINITY,
        ));
        assert_eq!(
            frame.geometry, state_frame.geometry,
            "两条布局链必须给出逐位相同的几何"
        );
        assert_eq!(frame.parent.len(), frame.geometry.len());
        assert_eq!(frame.text.len(), frame.geometry.len(), "文本列与几何同下标");
    }

    /// **判据 1 / 4**：命中链在生产路径里裁决「谁消费指针」；标签不消费。
    #[test]
    fn hit_chain_lets_labels_bubble_but_controls_consume() {
        let mut scene = Scene::new();
        let specs = vec![
            ControlSpec::label(LayoutId(0), "title"),
            ControlSpec::slider(LayoutId(1), "speed", 0.0, 2.0, 1.0),
        ];
        let mut hud = build(
            &mut scene,
            specs,
            HudMetrics::default_metrics(),
            |_, _, _| Vec::new(),
        );
        let label = hud.world_rect(LayoutId(0)).expect("标签有矩形");
        let label_point = hud
            .metrics
            .to_layout(Vec2::new(label.min.x + 1.0, label.center().y));
        // 标签在链上 ⇒ 但不消费（`on_intent(Pointer)` 返回 Continue）⇒ 链上无人消费。
        assert!(!hud.hit_chain_at(label_point).is_empty(), "标签在命中链上");
        assert!(!hud.pointer_is_consumed(label_point), "标签不得消费指针");

        let slider = hud.world_rect(LayoutId(1)).expect("滑块有矩形");
        let slider_point = hud
            .metrics
            .to_layout(Vec2::new(slider.min.x + 1.0, slider.center().y));
        assert!(hud.pointer_is_consumed(slider_point), "滑块必须消费指针");

        // 面板外：链为空 ⇒ 不消费。
        assert!(!hud.pointer_is_consumed(Point::new(-100.0, -100.0)));
    }

    /// **判据 4**：拖拽中卸载控件 ⇒ capture 显式取消并进诊断（不静默）。
    #[test]
    fn unmounting_the_dragged_control_cancels_capture_and_records_a_diagnostic() {
        use crate::domain::{StructureOp, WidgetId};
        let mut scene = Scene::new();
        let metrics = HudMetrics::default_metrics();
        let specs = vec![ControlSpec::slider(LayoutId(0), "speed", 0.0, 2.0, 1.0)];
        let mut hud = build(&mut scene, specs, metrics, |_, _, _| Vec::new());
        let rect = hud.world_rect(LayoutId(0)).expect("有矩形");
        let at = Vec2::new(rect.min.x + rect.size().x * 0.25, rect.center().y);

        let outcomes = hud.handle_world(SceneEvent::ViewportClick {
            layer: LayerId(0),
            scene_pos: at,
        });
        assert!(!outcomes.is_empty(), "按下必须产生 outcome");
        assert_eq!(hud.set().routed(), Some(LayoutId(0)), "capture 已落到滑块");

        let changed = hud.apply_structure_delta(&[StructureOp::Unmount { id: WidgetId(0) }]);
        assert!(changed, "卸载必须改变声明树");
        assert_eq!(hud.set().routed(), None, "capture 必须被显式取消");
        assert!(
            hud.errors()
                .iter()
                .any(|error| error.contains("capture 已显式取消")),
            "诊断必须可回执：{:?}",
            hud.errors()
        );
        assert!(hud.world_rect(LayoutId(0)).is_none(), "卸载后不再有该行");
    }

    /// **判据 1**：自定义 `SpecId` 注册后由同一渲染路径出命令（`Shape2D` 变体零改动）。
    #[test]
    fn custom_spec_draws_a_polyline_through_the_same_pipeline() {
        let decls = vec![WidgetDecl {
            id: LayoutId(0),
            spec: WidgetSpecId::new(SPARKLINE_SPEC),
            label: "curve".to_string(),
            role: "sparkline".to_string(),
            ..WidgetDecl::default()
        }];
        let mut runtime = runtime_with_factories();
        let layout = layout_widgets(
            &decls,
            &mut runtime,
            &mut ze_interact::solver::LayoutEngine::new(),
            ze_interact::Rect::from_min_size(Point::new(0.0, 0.0), 100.0, 0.0),
            &LayoutTokens::default(),
            &DrawEnv::default(),
        )
        .expect("自定义控件可布局");
        let metrics = Metrics::default();
        let ctx = DrawCtx {
            bounds: ze_interact::Rect::default(),
            clip: None,
            theme_id: &Theme::default_theme().id,
            themes: &ThemeTable::with_default(),
            metrics: &metrics,
            env: &DrawEnv::default(),
            z_base: ZOrder(HUD_TRACK_Z),
            hover: false,
            phase: WidgetPhase::FrameEnd,
        };
        let commands = runtime.draw_all(&layout.bounds, &ctx, None);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].shape.kind_name(), "polyline");
        assert!(commands[0].style.stroke.is_some(), "折线是描边不是填充");
    }

    /// 临时排查：克隆后的面板（`HudSystem` 每帧走这条路）仍能命中。
    #[test]
    fn cloned_hud_still_hits() {
        let mut scene = Scene::new();
        let metrics = HudMetrics::default_metrics();
        let specs = vec![ControlSpec::slider(LayoutId(0), "speed", 0.0, 2.0, 1.0)];
        let hud = build(&mut scene, specs, metrics, |_, _, _| Vec::new());
        let mut hud = hud.clone();
        let rect = hud.world_rect(LayoutId(0)).expect("有矩形");
        let at = Vec2::new(rect.min.x + rect.size().x * 0.8, rect.center().y);
        let outcomes = hud.handle_world(SceneEvent::ViewportClick {
            layer: LayerId(0),
            scene_pos: at,
        });
        assert!(!outcomes.is_empty(), "克隆后必须仍能命中：{outcomes:?}");
        hud.sync_from_set();
        let frame = hud.frame();
        let value = frame
            .value_of("slider", "speed")
            .expect("语义里有值")
            .parse::<f64>()
            .expect("数值");
        assert!(
            (value - 1.6).abs() < 1e-6,
            "Frame 语义值必须当帧刷新：{value}"
        );
    }

    /// **回归**：松手（`Released`）不产生 outcome，但必须清掉 capture。
    #[test]
    fn release_stops_the_knob_from_following_the_pointer() {
        let mut scene = Scene::new();
        let metrics = HudMetrics {
            origin: Vec2::new(-5.0, -2.0),
            width: 4.0,
            ..HudMetrics::default_metrics()
        };
        let specs = vec![ControlSpec::slider(LayoutId(0), "speed", 0.0, 2.0, 1.0)];
        let hud = build(&mut scene, specs, metrics, |_, _, _| Vec::new());
        let id = LayoutId(0);
        let rect = hud.world_rect(id).expect("有控件矩形");
        let at = |t: f32| Vec2::new(rect.min.x + rect.size().x * t, rect.center().y);

        let mut resources = Resources::new();
        resources.insert(HudState::new(hud));
        resources.insert(InputEvents::default());
        resources.insert(DirectorState::new());
        resources.insert(crate::domain::UiConfig::default());
        resources.insert(crate::animation::BindingTable::default());
        resources.insert(ze_scene_core::WidgetCommands::default());
        let mut scheduler = Scheduler::new(scene.into_world(), resources);
        scheduler.add_system(Phase::FrameEnd, HudSystem);

        run_frame(
            &mut scheduler,
            vec![SceneEvent::ViewportClick {
                layer: LayerId(0),
                scene_pos: at(0.25),
            }],
        );
        assert!((value_of(&scheduler, id) - 0.5).abs() < 1e-6);

        run_frame(
            &mut scheduler,
            vec![SceneEvent::ViewportRelease {
                layer: LayerId(0),
                scene_pos: at(0.25),
            }],
        );

        run_frame(
            &mut scheduler,
            vec![SceneEvent::ViewportHover {
                layer: LayerId(0),
                scene_pos: at(0.9),
            }],
        );
        assert!(
            (value_of(&scheduler, id) - 0.5).abs() < 1e-6,
            "松手后滑块仍在跟随指针：{}",
            value_of(&scheduler, id)
        );
    }
}
