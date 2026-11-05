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
#[derive(Debug, Clone)]
struct Row {
    id: LayoutId,
    kind: ControlKind,
    /// 滑块圆心（仅滑块）。
    knob: Option<ObjectId>,
    /// 单位坐标下的控件矩形（装配期算一次）。
    rect: ze_interact::Rect,
    /// **CK-g**：已烘焙的标签字形（一份数据两用：场景图元 + `DrawCommand`）。
    glyphs: Vec<Path2D>,
}

/// 场景里的一块自研面板。
#[derive(Debug, Clone)]
pub struct Hud {
    set: ControlSet,
    metrics: HudMetrics,
    rows: Vec<Row>,
    /// 本帧状态机产出的外部意图（**镜像**，供 debug 回执读取）。
    ///
    /// 为什么不给 `ze-interact::ControlOutcome` 加 serde：kernel 保持默认零依赖，
    /// debug 协议不该污染它（R8）。这里只做形状镜像，不含任何协议逻辑。
    last_outcomes: Vec<HudOutcome>,
    /// **CK-e**：本帧的回写诊断（未知控件名等）。空 = 没有异常。
    last_errors: Vec<String>,
    /// **CK-g**：主题 id（只存 id；颜色由 `Theme2D` 解析，诊断里回执 id）。
    theme: String,
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

/// **CK-e**：数值回写的 typed intent（名字 → 枚举在**边界**一次完成）。
///
/// 此前是 `match name.as_str()` + 未知名字 `eprintln!`：名字拼错不会红、
/// 诊断里也看不到。现在未知名字进 [`Hud::errors`]（回执可见），分发按枚举走。
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
    /// 解析控件名；未知 ⇒ `None`（调用方记诊断，不静默吞）。
    pub fn parse(name: &str) -> Option<Self> {
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

/// **CK-e**：激活类回写的 typed intent。
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
    pub fn parse(name: &str) -> Option<Self> {
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

    /// 值变化后滑块 / 单选组的指示圆 x（世界坐标）。
    fn knob_world_x(&self, id: LayoutId) -> Option<(ObjectId, f32)> {
        let row = self.rows.iter().find(|row| row.id == id)?;
        let knob = row.knob?;
        let value = self.set.value(id)?;
        let t = match row.kind {
            ControlKind::Slider { min, max } => {
                if (max - min).abs() < f64::EPSILON {
                    0.0
                } else {
                    ((value - min) / (max - min)).clamp(0.0, 1.0)
                }
            }
            // 单选组：指示圆落在选中段中心。
            ControlKind::RadioGroup { options } => {
                let count = options.len().max(1) as f64;
                ((value + 0.5) / count).clamp(0.0, 1.0)
            }
            _ => return None,
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

    /// 本帧状态机产出的意图（debug 回执读取；空 = 本帧没命中任何控件）。
    pub fn outcomes(&self) -> &[HudOutcome] {
        &self.last_outcomes
    }

    /// 清空意图（每帧开头调用；状态机本身只返回 `Vec`）。
    pub fn clear_outcomes(&mut self) {
        self.last_outcomes.clear();
        self.last_errors.clear();
    }

    /// **CK-e**：本帧回写诊断（未知控件名 / 参数写入被拒）。
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

    /// **CK-g**：本面板的主题 id（进 `Diagnostics.environment`）。
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// **CK-g**：控件声明 → 绘制命令（每条命令自带 `Style2D`）。
    ///
    /// 与场景图元**同一份**布局 / 字形数据：主题或 hover 只改命令里的颜色，
    /// 实体数不变。`Shape2D` 一个变体都没动。
    pub fn commands(&self) -> Vec<ze_render_ir::DrawCommand> {
        let theme = ze_scene_core::widget::Theme2D::default_theme();
        let routed = self.set.routed();
        let mut out = Vec::new();
        for row in &self.rows {
            let world = self.metrics.to_world(row.rect);
            let knob = row.knob.and_then(|_| self.knob_world_x(row.id)).map(|(_, x)| {
                Vec2::new(x, world.center().y)
            });
            let role = match row.kind {
                ControlKind::Slider { .. } => "slider",
                ControlKind::Button => "button",
                ControlKind::Label => "label",
                ControlKind::RadioGroup { .. } => "radio",
            };
            let name = self
                .set
                .specs()
                .iter()
                .find(|spec| spec.id == row.id)
                .map(|spec| spec.name.as_str())
                .unwrap_or("");
            let widget = ze_scene_core::widget::Widget2D::new(
                row.id.0,
                role,
                name,
                theme.id.clone(),
            );
            let ctx = ze_scene_core::widget::WidgetDrawContext {
                track: world,
                knob,
                knob_radius: self.metrics.knob_radius,
                hover: routed == Some(row.id),
                z: HUD_TRACK_Z,
                glyphs: row.glyphs.clone(),
            };
            out.extend(widget.draw(&ctx, &theme));
        }
        out
    }

    fn record_error(&mut self, message: String) {
        self.last_errors.push(message);
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
    // 行距 / 内边距**由 metrics 换算成布局单位**（否则 `metrics.gap` 只是注释里的数字，
    // 标签按它算偏移、布局却按 `ControlSet` 的默认值走 ⇒ 文字仍会压到上一行）。
    let unit = metrics.unit().max(f32::EPSILON);
    let gap_units = f64::from(metrics.gap / unit);
    let padding_units = f64::from(metrics.padding / unit);
    let mut set = ControlSet::new(specs).with_spacing(gap_units, padding_units);
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

        // 标签放在**行距**里（轨道上方、下一行下方），不压到相邻控件。
        //
        // 为什么要按 `gap` 算偏移：文字的高度约 `font * 1.3`，若直接顶在
        // `world.max.y` 上就会伸进上一行的轨道 —— 这正是「上方文字压在下一个按钮上」。
        let text_height = metrics.label_font_size * 1.3;
        let label_y = world.max.y + ((metrics.gap - text_height) * 0.5).max(0.0);
        // 标签：与轨道左端对齐（世界坐标），字号由 metrics 一处定义。
        let glyphs = bake_label(
            &spec_name(&set, placed.id),
            Vec2::new(world.min.x, label_y),
            metrics.label_font_size,
        );
        for glyph_path in &glyphs {
            scene
                .spawn_dyn(vec![
                    Box::new(Path(glyph_path.clone())),
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
                Some(t)
            }
            // 单选组：指示圆落在选中段中心。
            ControlKind::RadioGroup { options } => {
                let count = options.len().max(1) as f64;
                let value = set.value(placed.id).unwrap_or(0.0);
                Some((((value + 0.5) / count).clamp(0.0, 1.0)) as f32)
            }
            _ => None,
        };
        let knob = knob.map(|t| {
            scene
                .spawn_dyn(vec![
                    Box::new(Circle::new(
                        Vec2::new(world.min.x + world.size().x * t, world.center().y),
                        metrics.knob_radius,
                    )),
                    Box::new(FillSolid::new(Color32::new(228, 232, 244, 255))),
                    Box::new(ZOrder::new(HUD_KNOB_Z)),
                ])
                .expect("HUD：Circle / FillSolid 已登记")
        });

        rows.push(Row {
            id: placed.id,
            kind,
            knob,
            rect,
            glyphs,
        });
    }

    Hud {
        set,
        metrics,
        rows,
        last_outcomes: Vec::new(),
        last_errors: Vec::new(),
        theme: ze_scene_core::widget::Theme2D::default_theme().id,
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
            // 没有输入也要清掉上一帧的意图镜像：否则 debug 回执会把旧 outcome
            // 当成这一帧的（差分本来就没有变化，回执却"一直有变化"）。
            if let Some(hud) = ctx.get_mut::<HudState>().and_then(HudState::hud_mut) {
                if !hud.outcomes().is_empty() {
                    hud.clear_outcomes();
                }
            }
            return;
        }
        let Some(mut hud) = ctx.get_mut::<HudState>().and_then(HudState::hud_mut).cloned() else {
            return;
        };

        let mut outcomes = Vec::new();
        // 每帧重置镜像：`hud` 是从 `HudState` 克隆出来的，上一帧的意图不能留到这一帧。
        hud.clear_outcomes();
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
                let (id, kind) = match outcome {
                    ControlOutcome::ValueChanged { id, value } => {
                        (id, HudOutcomeKind::ValueChanged(value))
                    }
                    ControlOutcome::Activated { id } => (id, HudOutcomeKind::Activated),
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
                match kind {
                    HudOutcomeKind::ValueChanged(value) => {
                        let Some(intent) = ValueIntent::parse(&name) else {
                            hud.record_error(format!("未知数值控件 `{name}`"));
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
                        let Some(intent) = ActionIntent::parse(&name) else {
                            hud.record_error(format!("未知按钮 `{name}`"));
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

    /// **CK-g**：一个控件声明 ⇒ 一份命令列表；主题 / hover 只改颜色，不改条数。
    #[test]
    fn widget_commands_carry_their_own_style_and_theme() {
        let mut scene = Scene::new();
        let specs = vec![
            ControlSpec::slider(LayoutId(0), "speed", 0.0, 2.0, 1.0),
            ControlSpec::button(LayoutId(1), "play_pause"),
        ];
        let hud = build(&mut scene, specs, HudMetrics::default_metrics(), |_, _, _| {
            Vec::new()
        });
        let commands = hud.commands();
        // 两个控件：轨道 + 滑块（按钮没有滑块；没有烘焙字形）。
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].shape.kind_name(), "rect");
        assert_eq!(commands[1].shape.kind_name(), "circle");
        assert_eq!(commands[2].shape.kind_name(), "rect");
        assert!(!hud.theme().is_empty(), "主题 id 必须可回执");
    }

    /// **回归**：标签画在行距里，不得压到任何一行的轨道（"上方文字压在下一个按钮上"）。
    #[test]
    fn labels_sit_between_rows_without_overlapping_tracks() {
        let mut scene = Scene::new();
        let metrics = HudMetrics {
            origin: Vec2::new(-5.0, 0.0),
            width: 4.0,
            row_height: 0.36,
            gap: 0.29,
            padding: 0.14,
            label_font_size: 0.20,
            knob_radius: 0.11,
        };
        let specs: Vec<ControlSpec> = (0..4)
            .map(|index| ControlSpec::button(LayoutId(index), format!("b{index}")))
            .collect();
        let mut anchors = Vec::new();
        let hud = build(&mut scene, specs, metrics, |_, anchor, _| {
            anchors.push(anchor);
            Vec::new()
        });
        assert_eq!(anchors.len(), 4);
        let text_height = metrics.label_font_size * 1.3;
        for (index, anchor) in anchors.iter().enumerate() {
            let text_bottom = anchor.y;
            let text_top = anchor.y + text_height;
            for row in 0..4 {
                let rect = hud.world_rect(LayoutId(row)).expect("有控件矩形");
                let overlaps = text_bottom < rect.max.y && text_top > rect.min.y;
                assert!(
                    !overlaps,
                    "第 {index} 行标签 [{text_bottom:.3}, {text_top:.3}] 压到第 {row} 行轨道 \
                     [{:.3}, {:.3}]",
                    rect.min.y, rect.max.y
                );
            }
        }
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
