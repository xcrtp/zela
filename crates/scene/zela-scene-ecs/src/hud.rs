//! **CG-f**：自研 UI 的第一块可见面板 —— 三组控件全部是**场景图元**。
//!
//! # 它把什么接起来
//!
//! ```text
//! ControlSpec（声明）  →  ControlSet（ze-interact 状态机）
//!        ↓ 装配期一次                        ↓ 事件驱动
//! 场景图元（Rect / Circle / Path）       Frame（几何 + 语义 + 文本）
//! ```
//!
//! 标签用 `ze-prerender` 的直排文字引擎（ttf 轮廓 + `TextRun`）烘焙成 `Path`；
//! 轨道是 `Rect`、滑块是 `Circle` —— **host 只翻译输入，不画控件**。
//!
//! # 布局不在每帧
//!
//! 摆放只在**装配期**做一次（[`build`]）；运行时只改滑块圆心的 x（[`HudState`]），
//! 不重排。这与 CE-e 的面板同一条纪律。

use ze_interact::{
    Constraints, ControlEvent, ControlKind, ControlOutcome, ControlSet, ControlSpec, Frame,
    LayoutId, Point,
};
use ze_notion::{Color32, Path2D, Rect2D, Vec2};
use ze_render_ir::{FillSolid, StrokeSolid};
use ze_scene_core::{Circle, Path, Rect as SceneRect, ZOrder};
use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

use crate::Scene;
use crate::contract::ObjectId;
use crate::domain::{InputEvents, SceneEvent};
use ze_scene_contract::Scene as _;

/// 面板各层的绘制层级（与 CE-e 同一套：不声明层级时矩形会盖住圆与标签）。
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
}

/// 一个控件的场景图元。
#[derive(Debug, Clone, Copy)]
struct Row {
    id: LayoutId,
    kind: ControlKind,
    /// 滑块圆心（仅滑块）。
    knob: Option<ObjectId>,
    /// 单位坐标下的控件矩形（装配期算一次）。
    rect: ze_interact::Rect,
}

/// 场景里的一块自研面板。
#[derive(Debug, Clone)]
pub struct Hud {
    set: ControlSet,
    metrics: HudMetrics,
    rows: Vec<Row>,
}

impl Hud {
    /// 当前帧（几何 + 语义 + 文本）。文本层这里留空：`TextRun` 已烘焙成 `Path`。
    pub fn frame(&mut self) -> Frame {
        self.set.frame(self.constraints())
    }

    pub fn set(&self) -> &ControlSet {
        &self.set
    }

    pub fn value(&self, id: LayoutId) -> Option<f64> {
        self.set.value(id)
    }

    fn constraints(&self) -> Constraints {
        let width = f64::from(self.metrics.width)
            / f64::from(self.metrics.unit().max(f32::EPSILON)).max(1e-6);
        let height = self
            .rows
            .iter()
            .map(|row| f64::from(row.rect.height()))
            .sum::<f64>();
        Constraints::new(width, height)
    }

    /// 世界坐标 → 控件事件。落在面板外 ⇒ 仍然传（状态机会忽略）。
    pub fn handle_world(&mut self, event: SceneEvent) -> Vec<ControlOutcome> {
        let point = |pos: Vec2| self.metrics.to_layout(pos);
        match event {
            SceneEvent::ViewportHover { scene_pos, .. } => {
                self.set.handle(ControlEvent::Moved(point(scene_pos)))
            }
            SceneEvent::ViewportClick { scene_pos, .. } => {
                self.set.handle(ControlEvent::Pressed(point(scene_pos)))
            }
            SceneEvent::ViewportRelease { scene_pos, .. } => {
                self.set.handle(ControlEvent::Released(point(scene_pos)))
            }
            _ => Vec::new(),
        }
    }

    /// 值变化后滑块的圆心 x（世界坐标）。
    fn knob_world_x(&self, id: LayoutId) -> Option<(ObjectId, f32)> {
        let row = self.rows.iter().find(|row| row.id == id)?;
        let knob = row.knob?;
        let (min, max) = match row.kind {
            ControlKind::Slider { min, max } => (min, max),
            _ => return None,
        };
        let value = self.set.value(id)?;
        let t = if (max - min).abs() < f64::EPSILON {
            0.0
        } else {
            ((value - min) / (max - min)).clamp(0.0, 1.0)
        };
        let world = self.metrics.to_world(row.rect);
        Some((knob, world.min.x + world.size().x * t as f32))
    }

    /// 全部滑块的最新圆心（装配后的第一次同步）。
    pub fn knob_xs(&self) -> Vec<(ObjectId, f32)> {
        self.rows
            .iter()
            .filter_map(|row| self.knob_world_x(row.id))
            .collect()
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
    ///
    /// 布局只在装配期算过一次；这里只是换算单位（不改状态、不重排）。
    pub fn world_rect(&self, id: LayoutId) -> Option<Rect2D> {
        let row = self.rows.iter().find(|row| row.id == id)?;
        Some(self.metrics.to_world(row.rect))
    }
}

/// **装配期**：把一组控件摆成场景图元，返回可交互的 [`Hud`]。
///
/// `bake_label(text, anchor_world, font_size)` 由调用方实现（直排文字引擎 / typst）：
/// 返回若干条轮廓（一个字形一条）。
pub fn build(
    scene: &mut Scene,
    specs: Vec<ControlSpec>,
    metrics: HudMetrics,
    mut bake_label: impl FnMut(&str, Vec2, f32) -> Vec<Path2D>,
) -> Hud {
    let mut set = ControlSet::new(specs);
    // 布局只做一次：拿到每行的单位矩形。
    let frame = set.frame(Constraints::new(
        f64::from(metrics.width) / f64::from(metrics.unit().max(f32::EPSILON)).max(1e-6),
        f64::INFINITY,
    ));

    let mut rows = Vec::with_capacity(frame.geometry.len());
    for placed in &frame.geometry {
        let rect = placed.rect;
        let world = metrics.to_world(rect);
        let kind = set
            .specs()
            .iter()
            .find(|spec| spec.id == placed.id)
            .map(|spec| spec.kind)
            .unwrap_or(ControlKind::Label);

        scene
            .spawn_dyn(vec![
                Box::new(SceneRect(world)),
                Box::new(FillSolid::new(Color32::new(52, 54, 64, 255))),
                Box::new(ZOrder::new(HUD_TRACK_Z)),
            ])
            .expect("HUD：Rect / FillSolid 已登记");

        // 标签：放在轨道左侧（世界坐标），字号由 metrics 一处定义。
        for glyph_path in bake_label(
            &spec_name(&set, placed.id),
            Vec2::new(world.min.x, world.max.y + metrics.label_font_size * 0.5),
            metrics.label_font_size,
        ) {
            scene
                .spawn_dyn(vec![
                    Box::new(Path(glyph_path)),
                    Box::new(StrokeSolid::new(1.0, Color32::new(210, 214, 226, 255))),
                    Box::new(ZOrder::new(HUD_LABEL_Z)),
                ])
                .expect("HUD：Path / StrokeSolid 已登记");
        }

        let knob = match kind {
            ControlKind::Slider { min, max } => {
                let value = set.value(placed.id).unwrap_or(min);
                let t = if (max - min).abs() < f64::EPSILON {
                    0.0
                } else {
                    ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
                };
                let knob = scene
                    .spawn_dyn(vec![
                        Box::new(Circle::new(
                            Vec2::new(world.min.x + world.size().x * t, world.center().y),
                            metrics.knob_radius,
                        )),
                        Box::new(FillSolid::new(Color32::new(228, 232, 244, 255))),
                        Box::new(ZOrder::new(HUD_KNOB_Z)),
                    ])
                    .expect("HUD：Circle / FillSolid 已登记");
                Some(knob)
            }
            _ => None,
        };

        rows.push(Row {
            id: placed.id,
            kind,
            knob,
            rect,
        });
    }

    Hud {
        set,
        metrics,
        rows,
    }
}

fn spec_name(set: &ControlSet, id: LayoutId) -> String {
    set.specs()
        .iter()
        .find(|spec| spec.id == id)
        .map(|spec| spec.name.clone())
        .unwrap_or_default()
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

/// **HUD System**：消费视口事件 → 控件状态机 → 写回场景资源 + 移动滑块。
///
/// 复用 CE-e 的回写入口（`params::apply` / `DirectorState` / `UiConfig`），
/// 不新建第二套参数寻址。
pub struct HudSystem;

impl System for HudSystem {
    type Reads = (InputEvents, HudState);
    // 写者上限是 4 个一组（`ResourceTuple`）；参数注册表的值由 `params::apply`
    // 自己维护绑定表，HUD 不再同步第二份（避免两个真相）。
    type Writes = (
        HudState,
        crate::DirectorState,
        crate::domain::UiConfig,
        crate::animation::BindingTable,
    );

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        let events: Vec<SceneEvent> = match ctx.get::<InputEvents>() {
            Some(events) => events.iter().cloned().collect(),
            None => return,
        };
        if events.is_empty() {
            return;
        }
        let Some(mut hud) = ctx.get_mut::<HudState>().and_then(HudState::hud_mut).cloned() else {
            return;
        };

        let mut outcomes = Vec::new();
        for event in &events {
            outcomes.extend(hud.handle_world(event.clone()));
        }
        // **不能**按 `outcomes.is_empty()` 提前返回：`Released` / `Hover` 只改**路由状态**
        // （capture / focus），不产生 outcome。`hud` 是每帧从 `HudState` 克隆出来的，
        // 不写回就等于回滚 —— 松手后的捕获永远清不掉，滑块会一直跟随指针。
        if !outcomes.is_empty() {
            let mut director = ctx.get::<crate::DirectorState>().cloned().unwrap_or_default();
            let mut ui = ctx.get::<crate::domain::UiConfig>().cloned().unwrap_or_default();
            let mut table = ctx
                .get::<crate::animation::BindingTable>()
                .cloned()
                .unwrap_or_default();

            for outcome in outcomes {
                let id = match outcome {
                    ControlOutcome::ValueChanged { id, .. } | ControlOutcome::Activated { id } => id,
                };
                let Some(name) = hud
                    .set()
                    .specs()
                    .iter()
                    .find(|spec| spec.id == id)
                    .map(|spec| spec.name.clone())
                else {
                    continue;
                };
                match outcome {
                    ControlOutcome::ValueChanged { value, .. } => {
                        if name.contains(':') {
                            // 参数地址（CA 的注册表口径）：与 CE-e 同一条回写路径。
                            if let Err(error) =
                                crate::animation::params::apply(&mut table, &name, value as f32)
                            {
                                eprintln!("[zela] HUD 参数 `{name}` 写入被拒绝：{error}");
                            }
                        } else {
                            match name.as_str() {
                                "speed" => director.director.set_speed(value),
                                "seek" => director.director.seek(value),
                                other => eprintln!("[zela] HUD 未知滑块 `{other}`"),
                            }
                        }
                    }
                    ControlOutcome::Activated { .. } => match name.as_str() {
                        "play_pause" => director.director.toggle(),
                        "replay" => {
                            director.director.seek(0.0);
                            director.director.set_speed(1.0);
                        }
                        "show_grid" => ui.show_grid = !ui.show_grid,
                        "show_field" => ui.show_field = !ui.show_field,
                        "filter_gain" => ui.filter_gain = !ui.filter_gain,
                        "filter_distortion" => ui.filter_distortion = !ui.filter_distortion,
                        "filter_blur" => ui.filter_blur = !ui.filter_blur,
                        other => eprintln!("[zela] HUD 未知按钮 `{other}`"),
                    },
                }
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

        // 滑块圆心跟随取值（世界坐标写回）。
        for (knob, x) in hud.knob_xs() {
            if let Some(entity) = crate::contract::entity_of(knob) {
                if let Ok(mut circle) = ctx.world.get::<&mut Circle>(entity) {
                    circle.0.center.x = x;
                }
            }
        }
        if let Some(slot) = ctx.get_mut::<HudState>() {
            slot.0 = Some(hud);
        }
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

    /// **回归**：松手（`Released`）不产生 outcome，但必须清掉 capture —— 否则
    /// 之后的 hover 仍然被当作拖拽，滑块会一直跟随指针。
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
        let mut scheduler = Scheduler::new(scene.into_world(), resources);
        scheduler.add_system(Phase::FrameEnd, HudSystem);

        // 1) 按下 25%：值 = 0.5。
        run_frame(
            &mut scheduler,
            vec![SceneEvent::ViewportClick {
                layer: LayerId(0),
                scene_pos: at(0.25),
            }],
        );
        assert!((value_of(&scheduler, id) - 0.5).abs() < 1e-6);

        // 2) 松手：不产生 outcome，但要清掉 capture 并写回状态机。
        run_frame(
            &mut scheduler,
            vec![SceneEvent::ViewportRelease {
                layer: LayerId(0),
                scene_pos: at(0.25),
            }],
        );

        // 3) 松手后仅悬停到 90%：值不得再变。
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
