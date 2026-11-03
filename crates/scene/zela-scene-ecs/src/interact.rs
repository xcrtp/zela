//! 参数面板进场景（CE-e）：**真消费者**。
//!
//! # 它把四件事接在一起
//!
//! 命中（`ze_interact::LinearHitIndex`）+ 路由（`Router` 的 hover / focus / capture）
//! + CC 的作用域（路由键的 `scope`）+ CA 的参数注册表（`params::apply` 回写）。
//!
//! # 为什么面板必须是场景对象
//!
//! 若用 egui 画再贴成纹理，UI 就不真进场景（它是纹理，不是场景图元），
//! seek / 像素基线都存疑 —— 那是"看起来做完了"的假象。
//! 滑块 = 一条轨道（`Rect`）+ 一个圆（`Circle`），场景已有这些图元。
//!
//! # 布局只在装配期算一次
//!
//! `build_panel` 是纯装配函数；运行时只有**滑块圆心**随取值变化（一次写一个
//! `Circle.center`）。不在每帧重算布局（与"文档不进场景"同一思路）。
//!
//! # 作用域
//!
//! 每个面板属于一个 CC 作用域（`panel_a` / `panel_b`）。路由键带作用域 ⇒
//! 两个面板里"第 0 号滑块"是两个不同的目标，互不串扰。

use ze_interact::{
    HitId, HitIndex, HitTarget, LinearHitIndex, Point, Rect as HitRect, RouteKey, Router,
};
use ze_notion::{Path2D, Rect2D, Vec2};
use ze_property::{ParameterRegistry, PropertyValue};
use ze_render_ir::{FillSolid, StrokeSolid};

use crate::animation::{params, BindingTable};
use crate::domain::{InputEvents, SceneEvent};
use crate::scene::Scene;
use ze_scene_contract::Scene as _;

/// 面板各层的绘制层级。
///
/// # 为什么必须显式声明
///
/// 录制器按 `(图元, 样式)` 组合分组录制，且同 `ZOrder` 内保持稳定插入序 ——
/// 而组合的枚举顺序是 **Circle → Path → Line → Rect → Arrow**。也就是说：
/// 若不声明层级，**矩形一定画在圆与路径之上**，滑块会被轨道盖住、标签会被槽盖住。
///
/// 整层压在内容（默认 `0`）之上；槽 < 标签 < 滑块。
pub const PANEL_TRACK_Z: i32 = 100;
pub const PANEL_LABEL_Z: i32 = 110;
pub const PANEL_KNOB_Z: i32 = 120;

/// 面板的几何参数（装配期常量；只在 `build_panel` 里算一次）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelSpec {
    /// 面板左上角（场景坐标）。
    pub origin: Vec2,
    /// 轨道宽度。
    pub width: f32,
    /// 每行高度。
    pub row_height: f32,
    /// 行间距。
    pub gap: f32,
    /// 内边距。
    pub padding: f32,
    /// 滑块半径。
    pub knob_radius: f32,
    /// 左侧留给文字标签的宽度（0 = 不留）。
    pub label_width: f32,
    /// 标签字号（场景坐标单位）：必须**小于**行高，否则文字会串行。
    pub label_font_size: f32,
}

impl PanelSpec {
    pub fn new(origin: Vec2, width: f32) -> Self {
        Self {
            origin,
            width,
            row_height: 24.0,
            gap: 8.0,
            padding: 12.0,
            knob_radius: 5.0,
            label_width: 0.0,
            label_font_size: 8.0,
        }
    }

    /// 第 `index` 行的轨道矩形（场景坐标）。
    pub fn track(&self, index: usize) -> Rect2D {
        let y = self.row_y(index);
        Rect2D::from_min_size(
            Vec2::new(
                self.origin.x + self.padding + self.label_width,
                y,
            ),
            Vec2::new(
                self.width - self.padding * 2.0 - self.label_width,
                self.row_height,
            ),
        )
    }

    /// 第 `index` 行的上沿 y。
    pub fn row_y(&self, index: usize) -> f32 {
        self.origin.y + self.padding + index as f32 * (self.row_height + self.gap)
    }

    /// 标签的烘焙原点（左上角）：行内**垂直居中**。
    ///
    /// typst 的原点在文字左上方、文字向下生长 ⇒ 要让文字居中，就得把原点放在
    /// `行中心 - 字号/2`（否则小字会贴着行的下半边）。
    pub fn label_anchor(&self, index: usize) -> Vec2 {
        Vec2::new(
            self.origin.x + self.padding,
            self.row_y(index) + (self.row_height - self.label_font_size) * 0.5,
        )
    }
}

/// 面板里的一行：参数地址 + 取值范围 + 当前值 + 可选的文字标签几何。
#[derive(Debug, Clone)]
pub struct PanelEntry {
    /// 参数地址（CA 注册表口径，含作用域前缀）。
    pub address: String,
    /// 取值区间（滑块两端）。
    pub range: (f32, f32),
    /// 当前值（决定滑块初始位置；装配后由交互更新）。
    pub value: f32,
    /// 文字标签的**多条**轮廓（由调用方用 typst 烘焙：一个字形一条；空 = 不画）。
    pub labels: Vec<Path2D>,
}

impl PanelEntry {
    /// 新建：当前值取区间左端（与"滑块从左端开始"的旧行为一致）。
    pub fn new(address: impl Into<String>, range: (f32, f32)) -> Self {
        Self {
            address: address.into(),
            range,
            value: range.0,
            labels: Vec::new(),
        }
    }

    pub fn with_value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    pub fn with_labels(mut self, labels: Vec<Path2D>) -> Self {
        self.labels = labels;
        self
    }
}

/// 一个可拖动的滑块（装配期生成，运行时只读 + 写圆心）。
#[derive(Debug, Clone)]
pub struct SliderSpec {
    /// 命中目标 id（`HitId` 由装配顺序分配，确定性）。
    pub target: HitId,
    /// CC 作用域路径。
    pub scope: String,
    /// 参数地址。
    pub address: String,
    /// 轨道矩形（场景坐标）。
    pub track: Rect2D,
    /// 取值区间。
    pub range: (f32, f32),
    /// 滑块的场景对象（运行时写它的 `Circle.center`）。
    pub knob: ze_scene_contract::ObjectId,
}

impl SliderSpec {
    /// 当前指针 x → 取值（钳制到轨道两端）。
    pub fn value_at(&self, x: f32) -> f32 {
        let (lo, hi) = self.range;
        let width = (self.track.max.x - self.track.min.x).max(f32::EPSILON);
        let t = ((x - self.track.min.x) / width).clamp(0.0, 1.0);
        lo + (hi - lo) * t
    }

    /// 取值 → 轨道上的 x（画滑块圆心用）。
    pub fn x_at(&self, value: f32) -> f32 {
        x_on_track(self.track, self.range, value)
    }

    fn hit_rect(&self) -> HitRect {
        HitRect::new(
            Point::new(f64::from(self.track.min.x), f64::from(self.track.min.y)),
            Point::new(f64::from(self.track.max.x), f64::from(self.track.max.y)),
        )
    }
}

/// `取值 → 轨道上的 x`（`SliderSpec::x_at` 与装配期初值共用同一份映射）。
fn x_on_track(track: Rect2D, range: (f32, f32), value: f32) -> f32 {
    let (lo, hi) = range;
    let span = (hi - lo).abs().max(f32::EPSILON);
    let t = ((value - lo) / span).clamp(0.0, 1.0);
    track.min.x + (track.max.x - track.min.x) * t
}

/// 一组滑块（Resource）。命中索引在装配期建好，运行时只查。
#[derive(Debug, Clone, Default)]
pub struct InteractSet {
    sliders: Vec<SliderSpec>,
    index: LinearHitIndex,
}

impl InteractSet {
    pub fn new(sliders: Vec<SliderSpec>) -> Self {
        let index = LinearHitIndex::new(
            sliders
                .iter()
                .enumerate()
                .map(|(order, slider)| HitTarget::rect(slider.target, slider.hit_rect(), order as i32))
                .collect(),
        );
        Self { sliders, index }
    }

    pub fn sliders(&self) -> &[SliderSpec] {
        &self.sliders
    }

    pub fn is_empty(&self) -> bool {
        self.sliders.is_empty()
    }

    /// 场景坐标 → 最上层的滑块（z 降序由 `LinearHitIndex` 保证）。
    pub fn hit(&self, scene_pos: Vec2) -> Option<&SliderSpec> {
        let point = Point::from_f32(scene_pos.x, scene_pos.y);
        let id = *self.index.query(point).first()?;
        self.sliders.iter().find(|slider| slider.target == id)
    }
}

/// 路由状态（**不进产物**；靠重放事件流重建）。
#[derive(Debug, Clone, Default)]
pub struct InteractState {
    pub router: Router,
}

/// **场景内面板的一次事件消费**（由 `ParamPanelSystem` 调用，不单独注册 System）。
///
/// 拖拽语义（capture）：按下命中 → 锁定该滑块；期间指针移出仍写它；抬起解除。
/// 这条"故意不按命中结果走"的规则由 `Router` 表达（CE-c）。
///
/// 返回需要移动的滑块圆心 `(对象 id, 新 x)`；世界写回由调用方做
/// （`ParamPanelSystem` 与 egui 面板共用同一条回写路径，避免两个 System 抢写
/// 同一对资源）。
pub fn apply_viewport_events(
    events: &[SceneEvent],
    set: &InteractSet,
    state: &mut InteractState,
    table: &mut BindingTable,
    registry: &mut ParameterRegistry,
) -> Vec<(ze_scene_contract::ObjectId, f32)> {
    if set.is_empty() {
        return Vec::new();
    }
    let mut writes: Vec<(ze_scene_contract::ObjectId, f32, String, f32)> = Vec::new();
    for event in events {
        match event {
            SceneEvent::ViewportClick { scene_pos, .. } => {
                let hit = set.hit(*scene_pos);
                let key = hit.map(|slider| RouteKey::new(slider.scope.clone(), slider.target));
                state.router.pressed(key);
                if let Some(slider) = hit {
                    let value = slider.value_at(scene_pos.x);
                    writes.push((slider.knob, slider.x_at(value), slider.address.clone(), value));
                }
            }
            SceneEvent::ViewportHover { scene_pos, .. } => {
                let hit = set.hit(*scene_pos);
                let key = hit.map(|slider| RouteKey::new(slider.scope.clone(), slider.target));
                state.router.pointer_moved(key);
                // 只有 capture 期间才边移动边写（hover 本身不改参数）。
                if let Some(active) = state.router.capture() {
                    if let Some(slider) = set.sliders().iter().find(|slider| {
                        slider.target == active.target && slider.scope == active.scope
                    }) {
                        let value = slider.value_at(scene_pos.x);
                        writes.push((
                            slider.knob,
                            slider.x_at(value),
                            slider.address.clone(),
                            value,
                        ));
                    }
                }
            }
            SceneEvent::ViewportRelease { .. } => state.router.released(),
            _ => {}
        }
    }

    let mut moves = Vec::with_capacity(writes.len());
    for (knob, knob_x, address, value) in writes {
        match params::apply(table, &address, value) {
            Ok(()) => {
                let _ = registry.set_value(&address, PropertyValue::Scalar(value));
                moves.push((knob, knob_x));
            }
            Err(error) => {
                eprintln!("[zela] 参数 `{address}` 写入被拒绝：{error}");
            }
        }
    }
    moves
}

/// **装配期**：把一组参数摆成面板（场景对象），返回可命中的滑块表。
///
/// 每个条目生成：
/// - 轨道：`Rect` + `FillSolid`（深色槽）；
/// - 滑块：`Circle` + `FillSolid`（亮色圆，运行时只改 `center.x`）；
/// - 标签（可选）：`Path` + `StrokeSolid`（调用方用 typst 烘焙成轮廓）。
///
/// 返回值顺序 = 条目顺序（确定性）；`HitId` 也从 0 递增分配。
pub fn build_panel(
    scene: &mut Scene,
    spec: &PanelSpec,
    scope: &str,
    entries: &[PanelEntry],
) -> InteractSet {
    let mut sliders = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let track = spec.track(index);
        // 层级必须显式：录制器的组合顺序是 Circle → Path → Rect，
        // 不声明层级时矩形会盖住圆与标签（见 `PANEL_TRACK_Z` 的文档）。
        scene
            .spawn_dyn(vec![
                Box::new(ze_scene_core::Rect(track)),
                Box::new(FillSolid::new(ze_notion::Color32::new(52, 54, 64, 255))),
                Box::new(ze_scene_core::ZOrder::new(PANEL_TRACK_Z)),
            ])
            .expect("Rect / FillSolid 已登记");

        if !entry.labels.is_empty() {
            for label in &entry.labels {
                scene
                    .spawn_dyn(vec![
                        Box::new(ze_scene_core::Path(label.clone())),
                        Box::new(StrokeSolid::new(
                            1.0,
                            ze_notion::Color32::new(210, 214, 226, 255),
                        )),
                        Box::new(ze_scene_core::ZOrder::new(PANEL_LABEL_Z)),
                    ])
                    .expect("Path / StrokeSolid 已登记");
            }
        }

        let value_x = x_on_track(track, entry.range, entry.value);
        let knob = scene
            .spawn_dyn(vec![
                Box::new(ze_scene_core::Circle::new(
                    (value_x, track.center().y),
                    spec.knob_radius,
                )),
                Box::new(FillSolid::new(ze_notion::Color32::new(228, 232, 244, 255))),
                Box::new(ze_scene_core::ZOrder::new(PANEL_KNOB_Z)),
            ])
            .expect("Circle / FillSolid 已登记");

        sliders.push(SliderSpec {
            // `HitId` 取滑块的场景 id：全局唯一 ⇒ 多个面板合并成一个 `InteractSet`
            // 也不会串号（两个面板各自的 0 号滑块必须是两个目标）。
            target: HitId(knob.index),
            scope: scope.to_string(),
            address: entry.address.clone(),
            track,
            range: entry.range,
            knob,
        });
    }
    InteractSet::new(sliders)
}

/// 面板左侧标签预留宽度（世界单位；默认口径的一部分）。
pub const LABEL_WIDTH: f32 = 1.8;

/// **默认尺寸口径**（世界单位，与 `View::bounds` 同一坐标系）。
///
/// 场景坐标是像素还是世界单位由作品决定；这里是本仓库作品使用的口径
/// （面板宽 6 世界单位、行高 0.55、标签字号 0.30）。调用方要换单位就自己构造 `PanelSpec`。
pub fn default_panel_metrics() -> PanelSpec {
    PanelSpec {
        origin: Vec2::ZERO,
        width: 1.0,
        row_height: 0.55,
        gap: 0.20,
        padding: 0.25,
        knob_radius: 0.16,
        label_width: LABEL_WIDTH,
        // 字号必须明显小于行高（0.55）：0.9 会把标签顶出行外、并压到相邻行。
        label_font_size: 0.30,
    }
}

/// **装配期**：为场景里每个 `View` 生成一个参数面板。
///
/// `views`：View 对象 → CC 作用域路径；`scope_ordinals`：下标 = 作用域序号
/// （`BindingTable::scopes()`）。每个 View 只吸收**属于该作用域**的标量参数
/// （地址前缀 = 作用域序号）；其余作用域的控件不会出现在这一面上。
///
/// `metrics` 提供面板的**尺寸口径**（原点与宽度会被 View 的 `bounds` 覆盖）：
/// 场景坐标是像素还是世界单位由调用方决定，本函数不猜。
///
/// `bake_label(text, anchor, font_size)` 由调用方实现（typst / 占位引擎）：
/// 返回该文字的**多条轮廓**（一个字形一条）。字号由 `metrics` 给出（一处定义），
/// 面板只负责摆放，不认排版。
pub fn build_panels_for_views(
    scene: &mut Scene,
    registry: &ParameterRegistry,
    views: &[(ze_scene_contract::ObjectId, String)],
    scope_ordinals: &[String],
    metrics: PanelSpec,
    mut bake_label: impl FnMut(&str, Vec2, f32) -> Vec<Path2D>,
) -> InteractSet {
    use ze_property::Property;
    use ze_scene_contract::SceneRead as _;

    let mut all: Vec<SliderSpec> = Vec::new();
    for (view_id, scope_path) in views {
        let Some(view) = scene.with::<ze_scene_core::View, _>(*view_id, |view| *view) else {
            continue;
        };
        let Some(ordinal) = scope_ordinals.iter().position(|path| path == scope_path) else {
            continue;
        };
        let mut spec = metrics;
        spec.origin = view.bounds.min;
        spec.width = view.bounds.size().x;

        let mut entries = Vec::new();
        for index in 0..registry.len() {
            let Some(entry) = registry.at(index) else {
                continue;
            };
            let Some((prefix, _)) = entry.address.split_once(':') else {
                continue;
            };
            if prefix.parse::<usize>() != Ok(ordinal) {
                continue;
            }
            let Property::Scalar(config) = entry.property else {
                continue;
            };
            let anchor = spec.label_anchor(entries.len());
            entries.push(
                PanelEntry::new(entry.address, (config.min, config.max))
                    .with_value(config.value)
                    .with_labels(bake_label(&config.label, anchor, spec.label_font_size)),
            );
        }
        if entries.is_empty() {
            continue;
        }
        let set = build_panel(scene, &spec, scope_path, &entries);
        all.extend(set.sliders().iter().cloned());
    }
    InteractSet::new(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> PanelSpec {
        PanelSpec::new(Vec2::new(0.0, 0.0), 120.0)
    }

    /// 两个面板并排：**拖 A 不影响 B**（CE-e 主判据）。
    #[test]
    fn dragging_one_panel_does_not_touch_the_other() {
        let mut scene = Scene::new();
        let a = build_panel(
            &mut scene,
            &spec(),
            "panel_a",
            &[PanelEntry::new("0:state:0000", (0.0, 1.0))],
        );
        let b = build_panel(
            &mut scene,
            &PanelSpec::new(Vec2::new(200.0, 0.0), 120.0),
            "panel_b",
            &[PanelEntry::new("1:state:0000", (0.0, 1.0))],
        );
        assert_eq!(a.sliders().len(), 1);
        assert_eq!(b.sliders().len(), 1);
        assert_ne!(
            a.sliders()[0].target, b.sliders()[0].target,
            "两个面板的滑块必须是不同目标（HitId 取场景对象 id）"
        );
        assert_ne!(
            a.sliders()[0].address, b.sliders()[0].address,
            "两个面板的地址必须在各自作用域下"
        );

        let track_a = a.sliders()[0].track;
        let mid_a = track_a.min.x + (track_a.max.x - track_a.min.x) * 0.75;
        let hit_a = a.hit(Vec2::new(mid_a, track_a.center().y)).expect("命中 A");
        assert_eq!(hit_a.scope, "panel_a");
        assert!(
            b.hit(Vec2::new(mid_a, track_a.center().y)).is_none(),
            "A 的轨道不在 B 的命中范围内"
        );
        assert!((hit_a.value_at(mid_a) - 0.75).abs() < 1e-5);
    }

    /// 路由键带作用域：两个面板同号目标不会串。
    #[test]
    fn route_keys_are_scoped() {
        let a_key = RouteKey::new("panel_a", HitId(0));
        let b_key = RouteKey::new("panel_b", HitId(0));
        assert_ne!(a_key, b_key);
    }

    /// **层级**：滑块必须画在轨道之上、标签之上。
    ///
    /// 录制器的组合顺序是 Circle → Path → Rect，同 `ZOrder` 内稳定保持插入序 ⇒
    /// 不显式声明层级时，轨道矩形一定会盖住滑块与标签。
    #[test]
    fn knob_and_label_draw_above_the_track() {
        use ze_scene_contract::SceneRead as _;

        let mut scene = Scene::new();
        let set = build_panel(
            &mut scene,
            &spec(),
            "panel_a",
            &[PanelEntry::new("0:state:0000", (0.0, 1.0))],
        );
        let knob = set.sliders()[0].knob;
        assert_eq!(
            scene.with::<ze_scene_core::ZOrder, _>(knob, |z| z.0),
            Some(PANEL_KNOB_Z),
            "滑块必须显式落在滑块层"
        );

        let mut layers = Vec::new();
        scene.for_each::<ze_scene_core::ZOrder>(|_, z| layers.push(z.0));
        layers.sort_unstable();
        assert_eq!(
            layers,
            vec![PANEL_TRACK_Z, PANEL_KNOB_Z],
            "轨道与滑块各一层；标签未烘焙时不出对象"
        );
        assert!(PANEL_TRACK_Z < PANEL_LABEL_Z && PANEL_LABEL_Z < PANEL_KNOB_Z);
    }

    /// **端到端**：`ParamPanelSystem` 只写被按下的那个面板。
    #[test]
    fn interact_system_writes_only_the_captured_panel() {
        use crate::animation::{params as param_ops, Binding, BindingDriver, CircleRadius};
        use crate::animation::ResetBehavior;
        use crate::Curve;
        use ze_scene_contract::ObjectId;
        use ze_time::{ClockForest, ClockNode};
        use zela_schedule::resource::Resources;
        use zela_schedule::scheduler::{Phase, Scheduler};
        use std::sync::Arc;

        let mut table = BindingTable::default();
        let scope_a = table.declare_scope("panel_a");
        let scope_b = table.declare_scope("panel_b");
        let mut forest = ClockForest::default();
        let clock = forest.insert(ClockNode::new(None)).unwrap();
        for (scope, object) in [
            (scope_a, ObjectId::new(0, 1)),
            (scope_b, ObjectId::new(1, 1)),
        ] {
            table.add(Binding {
                clock,
                scope,
                priority: 0,
                driver: BindingDriver::Curve(Curve::Sin {
                    base: 0.5,
                    amp: 1.0,
                    omega: 1.0,
                }),
                target: Arc::new(CircleRadius { id: object }),
                predicate: None,
                active: None,
                reset: ResetBehavior::Hold,
                initial: None,
                state_plans: Vec::new(),
                seek_kind: crate::SeekKind::O1,
            });
        }
        let registry = param_ops::build_registry(&table);
        // 每条 `Sin` 曲线登记 base / amp / omega 三个参数 ⇒ 两个作用域共 6 条。
        assert_eq!(registry.len(), 6, "两个作用域 × 三个曲线字段");
        // 地址前缀是**作用域序号**（`panel_a` = 1、`panel_b` = 2），不是路径。
        let find = |binding: &str, field: &str| {
            (0..registry.len())
                .filter_map(|index| registry.at(index))
                .map(|entry| entry.address.to_string())
                .find(|address| address.contains(binding) && address.ends_with(field))
                .unwrap_or_else(|| panic!("找不到 {binding} 的 {field}"))
        };
        let address_a = find(":curve:0000:", ":base");
        let address_b = find(":curve:0001:", ":base");

        let spec_a = spec();
        let spec_b = PanelSpec::new(Vec2::new(200.0, 0.0), 120.0);
        let mut scene = Scene::new();
        let a = build_panel(
            &mut scene,
            &spec_a,
            "panel_a",
            &[PanelEntry::new(address_a.clone(), (0.0, 1.0))],
        );
        let b = build_panel(
            &mut scene,
            &spec_b,
            "panel_b",
            &[PanelEntry::new(address_b.clone(), (0.0, 1.0))],
        );
        let set = InteractSet::new(
            a.sliders()
                .iter()
                .chain(b.sliders().iter())
                .cloned()
                .collect(),
        );

        // 按 A 的 75% 位置（B 完全在别处）。
        let track = a.sliders()[0].track;
        let press = Vec2::new(track.min.x + (track.max.x - track.min.x) * 0.75, track.center().y);
        let mut resources = Resources::new();
        resources.insert(InputEvents::from_events(vec![SceneEvent::ViewportClick {
            layer: ze_render_ir::LayerId(0),
            scene_pos: press,
        }]));
        resources.insert(set);
        resources.insert(table);
        resources.insert(registry);
        resources.insert(InteractState::default());

        let mut sched = Scheduler::new(scene.into_world(), resources);
        sched.add_system(Phase::LogicUpdate, crate::input::ParamPanelSystem);
        sched.init_resources();
        sched.run_frame(0.016);

        let table = sched.resources().get::<BindingTable>().expect("写回绑定表");
        let base = |index: usize| match &table.bindings[index].driver {
            BindingDriver::Curve(Curve::Sin { base, .. }) => *base,
            _ => panic!("必须是曲线绑定"),
        };
        assert!(
            (base(0) - 0.75).abs() < 0.02,
            "panel_a 必须被写到 75%：{}",
            base(0)
        );
        assert!(
            (base(1) - 0.5).abs() < 1e-6,
            "panel_b 不得被改动：{}",
            base(1)
        );
    }
}
