//! **CL-b**：表达层定型 —— 一个控件 = 一份纯值声明 + 一个 `dyn` 实现 + 一段命令列表。
//!
//! # 为什么在这里，为什么是 feature
//!
//! 运行期资源（`WidgetRuntime`）与 `Frame` / `Control` / `Layout` / `Hit` / `Text` 同层，
//! 所以落在本 crate（不新建 crate）。但 `draw` 的输出是 [`ze_render_ir::DrawCommand`]，
//! 它会把 `glam` 间接拉进来 —— 而本 crate 默认零依赖的约定不能破。
//! 因此整层放在 **`widgets` feature** 后面：默认构建仍是零依赖；启用它的宿主
//! （`zela-scene-ecs`）显式承担这份依赖。
//!
//! # 三层落物
//!
//! ```text
//! 声明   WidgetDecl { id, spec, parent, props }      纯值，进产物
//! 编译   TreeIndex / revision（ze-graph-algo）        派生态，回放端重算
//! 运行   WidgetRuntime { BTreeMap<WidgetId, Box<dyn Widget2D>> }   派生态
//! ```
//!
//! # 三条反面约束（本批新增）
//!
//! 1. **不把 `Box<dyn>` 注册成 ECS 组件**：它会撞 `Component` 的 `Clone` blanket impl、
//!    封闭白名单，以及「组件要进产物而 dyn 不可序列化」的死结。dyn 只住 Resource。
//! 2. **不做 CSS 式全局样式表**：token 扁平、按 `role` 查表，不做层叠 / specificity。
//! 3. **不给 ctx 开反查其他 Widget 的入口**：签名里没有 `&World` / `&mut`；
//!    一查就退回树递归，CK 的 Euler 切片白做。

use std::collections::BTreeMap;

use crate::Point;
use crate::hit::Rect;
use crate::layout::Size;
use crate::layout::{Constraints, LayoutId};
use ze_render_ir::{DrawCommand, Shape2D, Style2D, ZOrder};

/// 控件在声明期的稳定 id（与 `Frame::geometry` 同口径；内容寻址，不是分配序）。
pub type WidgetId = LayoutId;

/// 控件规格（工厂表的键）。**独立命名空间**，不与 `ContentOriginKind` / `OpRef` 共用。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct WidgetSpecId(pub String);

impl WidgetSpecId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for WidgetSpecId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 布局策略（默认两遍扫描；`kasuari` 是同一接口下的第二个实现）。
///
/// CL-R 把定义搬到 [`crate::solver`]（那里也放 `LayoutTokens` 与约束形状），
/// 这里保持原路径可用。
pub use crate::solver::{LayoutTokens, StrategyId};

/// 主题 id（进 `Diagnostics.environment`，不进 payload）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ThemeId(pub String);

/// **绘制 token**：只改命令列表，不改 measure 结果（不进 revision）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaintTokens {
    pub track: [u8; 4],
    pub hover: [u8; 4],
    pub label: [u8; 4],
    pub knob: [u8; 4],
}

/// 一套解析后的主题（装配期常量；`MeasureCtx` 只拿 `layout`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Theme {
    pub id: ThemeId,
    pub layout: LayoutTokens,
    pub paint: PaintTokens,
}

impl Theme {
    pub fn default_theme() -> Self {
        Self {
            id: ThemeId("zela.default".to_string()),
            layout: LayoutTokens {
                row_height: 20.0,
                gap: 4.0,
                padding: 4.0,
                font_size: 12.0,
                knob_radius: 6.0,
            },
            paint: PaintTokens {
                track: [52, 54, 64, 255],
                hover: [68, 71, 84, 255],
                label: [210, 214, 226, 255],
                knob: [228, 232, 244, 255],
            },
        }
    }
}

/// 主题表：扁平 token 按 id 查表（不做层叠 / specificity）。
///
/// 默认表**自带**默认主题 ⇒ `get_or_default` 一定能借出 `&Theme`（不 clone、不 Option 传染）。
#[derive(Debug, Clone)]
pub struct ThemeTable {
    default: Theme,
    themes: BTreeMap<ThemeId, Theme>,
}

impl Default for ThemeTable {
    fn default() -> Self {
        let default = Theme::default_theme();
        let mut themes = BTreeMap::new();
        themes.insert(default.id.clone(), default.clone());
        Self { default, themes }
    }
}

impl ThemeTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, theme: Theme) {
        self.themes.insert(theme.id.clone(), theme);
    }

    pub fn get(&self, id: &ThemeId) -> Option<&Theme> {
        self.themes.get(id)
    }

    /// 查不到 ⇒ 默认主题（**不是**「没主题」：绘制永远有确定的 token）。
    pub fn get_or_default(&self, id: &ThemeId) -> &Theme {
        self.themes.get(id).unwrap_or(&self.default)
    }

    /// 主题表变化 ⇒ 命令列表可能变化（layout token 是否变化由 revision 判定）。
    pub fn revision(&self) -> u64 {
        let mut hash = Theme::default_theme().layout.revision();
        for (id, theme) in &self.themes {
            for byte in id.0.bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            hash ^= theme.layout.revision();
        }
        hash
    }
}

/// 绘制环境（CH-e `Diagnostics.environment` 的输入侧镜像）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrawEnv {
    pub scale_factor: f32,
    pub viewport: Rect,
    /// 字体 face 清单（只作解释，不参与换算）。
    pub faces: Vec<String>,
}

/// **宿主换算**：布局单位 → 世界单位（原点 + 缩放）。
///
/// 命中与绘制都走同一份换算：HUD 的 `HudMetrics::unit()` 就是这里的 `scale`。
/// 它属于**宿主**，不进产物；进 `DrawCtx` 是因为绘制发生在世界坐标里。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// 1 布局单位 = 多少世界单位。
    pub scale: f64,
    /// 布局原点在世界坐标中的位置。
    pub origin: Point,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            scale: 1.0,
            origin: Point::new(0.0, 0.0),
        }
    }
}

impl Metrics {
    pub fn new(scale: f64, origin: Point) -> Self {
        Self { scale, origin }
    }

    /// 布局矩形 → 世界矩形。
    pub fn to_world(&self, rect: Rect) -> Rect {
        Rect::new(
            Point::new(
                self.origin.x + rect.min.x * self.scale,
                self.origin.y + rect.min.y * self.scale,
            ),
            Point::new(
                self.origin.x + rect.max.x * self.scale,
                self.origin.y + rect.max.y * self.scale,
            ),
        )
    }

    /// 布局单位 → 世界单位的仿射（字形轮廓整体映射，不再逐点搬）。
    pub fn to_transform(&self) -> ze_notion::Transform2D {
        ze_notion::Transform2D::scale(ze_notion::Vec2::splat(self.scale as f32)).compose(
            ze_notion::Transform2D::translate(ze_notion::Vec2::new(
                self.origin.x as f32,
                self.origin.y as f32,
            )),
        )
    }
}

/// 回溯阶段上下文：还没有 bounds。
pub struct MeasureCtx<'a> {
    pub constraints: Constraints,
    pub layout: &'a LayoutTokens,
    pub env: &'a DrawEnv,
}

/// 前推之后上下文：拿到最终分配。
pub struct DrawCtx<'a> {
    pub bounds: Rect,
    pub clip: Option<Rect>,
    /// 本控件要用的主题 id —— 去 `themes` 里查（不做层叠）。
    pub theme_id: &'a ThemeId,
    pub themes: &'a ThemeTable,
    pub metrics: &'a Metrics,
    pub env: &'a DrawEnv,
    pub z_base: ZOrder,
    /// 指针是否悬停在本控件上（只改颜色，不改命令条数）。
    pub hover: bool,
    /// 只读相位标签，禁跨相位副作用。
    pub phase: WidgetPhase,
}

impl<'a> DrawCtx<'a> {
    /// 解析后的主题（查不到 ⇒ 默认主题）。
    pub fn theme(&self) -> &'a Theme {
        self.themes.get_or_default(self.theme_id)
    }
}

/// 意图处理上下文：命中部位 + 当前 bounds（控件自处理，不再按名字解析）。
pub struct IntentCtx<'a> {
    pub part: HitPart,
    pub bounds: Rect,
    pub metrics: &'a Metrics,
    pub phase: WidgetPhase,
}

/// 控件相位（只读标签）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetPhase {
    Assembly,
    Input,
    LogicUpdate,
    FrameEnd,
}

/// 控件的语义状态（纯值；由宿主写进 `Frame.semantics`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetState {
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub focusable: bool,
}

/// 命中部位（轨道 / 圆钮 / 字符盒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitPart {
    Track,
    Knob,
    Body,
    Char(usize),
}

/// 控件自处理的 typed intent（不再 `parse(name)`）。
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Value(f64),
    Activate,
    Pointer(Point),
}

/// 消费 / 继续（与命中的 [`crate::HitFlow`] 同一语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Consume,
    Continue,
}

/// 一个控件的**声明**（纯值；本模块只定义形状，组件在场景层）。
///
/// CL-R：补 `strategy` / `constraints` 并派生 serde —— 声明**进产物**，
/// 运行期 dyn 表与求解结果不进（见 `xtask` 的派生态门禁）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetDecl {
    pub id: WidgetId,
    pub spec: WidgetSpecId,
    pub parent: Option<WidgetId>,
    pub label: String,
    pub role: String,
    pub value: f64,
    /// 取值区间（滑块 / 单选组用；默认 0..0 = 无区间）。
    pub value_min: f64,
    pub value_max: f64,
    /// 该控件所属布局策略（默认 `two-pass`；同一棵树里混用策略是显式错误）。
    pub strategy: StrategyId,
    /// 声明期约束（跨容器关系上浮到最近共同祖先，由布局驱动统一求解）。
    pub constraints: crate::solver::ConstraintDecl,
}

/// 一段已烘焙的字形轮廓（**布局单位**；绘制时按 `DrawCtx.metrics` 映射到世界）。
///
/// CL-R 由「折线 + closed」换成 `Path2D`：字形是曲线，退回折线会改观感（也就改了像素基线）。
/// `Shape2D` 变体一个都没动 —— 仍然发 `Shape2D::Path`。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphOutline {
    pub path: ze_notion::Path2D,
}

impl GlyphOutline {
    pub fn from_polyline(points: &[Point], closed: bool) -> Self {
        let points: Vec<ze_notion::Vec2> = points
            .iter()
            .map(|p| ze_notion::Vec2::new(p.x as f32, p.y as f32))
            .collect();
        Self {
            path: ze_notion::Path2D::from_polyline(&points, closed),
        }
    }

    /// 字符格（布局单位；由轮廓包围盒派生）。
    pub fn cell(&self) -> Option<Rect> {
        self.path.compute_bounds().map(|bounds| {
            Rect::new(
                Point::new(f64::from(bounds.min.x), f64::from(bounds.min.y)),
                Point::new(f64::from(bounds.max.x), f64::from(bounds.max.y)),
            )
        })
    }
}

/// 工厂入参（纯值；控件自己的 props）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetProps {
    /// 规格 id：**绘制按它分派**（不再看 `role` 字符串）。
    pub spec: WidgetSpecId,
    pub label: String,
    pub role: String,
    pub value: f64,
    pub value_min: f64,
    pub value_max: f64,
    pub glyphs: Vec<GlyphOutline>,
}

/// 控件实现（object-safe）。
pub trait Widget2D: Send + Sync + 'static {
    /// 回溯上报固有需求。
    fn measure(&self, ctx: &MeasureCtx<'_>) -> Size;
    /// 前推分配。
    fn set_bounds(&mut self, bounds: Rect);
    fn bounds(&self) -> Rect;
    /// 纯值语义（进 `Frame.semantics`）。
    fn state(&self) -> WidgetState;
    /// 纯函数绘制：每条命令自带 `Style2D`。
    fn draw(&self, ctx: &DrawCtx<'_>) -> Vec<DrawCommand>;
    /// 文本层（可选）：由已烘焙字形派生字符盒，不新建实体。
    fn text_run(&self) -> Option<crate::TextRun> {
        None
    }
    /// 命中部位（可选）。
    fn hit_part(&self, _point: Point) -> Option<HitPart> {
        None
    }
    /// 控件自处理意图（按 `HitPart` + `IntentCtx`，不再 `parse(name)`）；默认继续冒泡。
    fn on_intent(&mut self, _intent: &Intent, _ctx: &IntentCtx<'_>) -> Verdict {
        Verdict::Continue
    }
}

/// 容器的声明契约（平级于 [`Widget2D`]，**不持有子对象**）。
pub trait Container2D: Send + Sync + 'static {
    /// 子声明（稳定序）。
    fn child_decls(&self) -> &[WidgetDecl];
    /// 沿 Euler 连续切片汇总（回溯）。
    fn measure_children(&self, slice: &[Size], constraints: Constraints) -> Size;
    /// 前推分配。
    fn arrange_children(&self, bounds: Rect) -> Vec<(WidgetId, Rect)>;
    /// 命中链冒泡裁决。
    fn bubble(&self, _chain: &[crate::HitId], _intent: &Intent) -> Verdict {
        Verdict::Continue
    }
}

/// 工厂：纯函数，从 props 造一个 dyn 控件。
pub type WidgetFactory = fn(&WidgetProps) -> Box<dyn Widget2D>;

/// 运行期控件表（Resource；**零序列化**、不持编译索引）。
///
/// 存 `props` 是为了 `Clone`：`Box<dyn>` 不能克隆，但「用同一份 props 重跑工厂」可以
/// （宿主每帧克隆 `Hud` 时走这条路）。
#[derive(Default)]
pub struct WidgetRuntime {
    widgets: BTreeMap<WidgetId, Box<dyn Widget2D>>,
    factories: BTreeMap<WidgetSpecId, WidgetFactory>,
    props: BTreeMap<WidgetId, WidgetProps>,
    /// 内容引擎按 id 注入的已烘焙字形（布局单位）。
    glyphs: BTreeMap<WidgetId, Vec<GlyphOutline>>,
}

impl std::fmt::Debug for WidgetRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetRuntime")
            .field("widgets", &self.widgets.keys().collect::<Vec<_>>())
            .field("factories", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Clone for WidgetRuntime {
    fn clone(&self) -> Self {
        let widgets = self
            .props
            .iter()
            .filter_map(|(&id, props)| {
                self.factories
                    .get(&props.spec)
                    .map(|factory| (id, factory(props)))
            })
            .collect();
        Self {
            widgets,
            factories: self.factories.clone(),
            props: self.props.clone(),
            glyphs: self.glyphs.clone(),
        }
    }
}

impl WidgetRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册规格工厂（未注册的 `WidgetSpecId` 在 `rebuild` 时显式报错）。
    pub fn register(&mut self, spec: impl Into<String>, factory: WidgetFactory) {
        self.factories.insert(WidgetSpecId::new(spec), factory);
    }

    pub fn has_spec(&self, spec: &WidgetSpecId) -> bool {
        self.factories.contains_key(spec)
    }

    /// 注入一个控件已烘焙的字形（布局单位；由内容引擎在装配期给出）。
    pub fn set_glyphs(&mut self, id: WidgetId, glyphs: Vec<GlyphOutline>) {
        self.glyphs.insert(id, glyphs);
    }

    pub fn glyphs(&self, id: WidgetId) -> &[GlyphOutline] {
        self.glyphs.get(&id).map(Vec::as_slice).unwrap_or_default()
    }

    /// 把值写回控件：**props 与 dyn 实例同步**。
    ///
    /// 为什么必须同步 props：`Clone` 是用 props 重跑工厂实现的；只改实例不改 props，
    /// 克隆出来的面板会退回旧值（宿主每帧克隆一次，症状是「拖了没反应」）。
    pub fn apply_value(&mut self, id: WidgetId, value: f64) -> bool {
        let Some(props) = self.props.get_mut(&id) else {
            return false;
        };
        props.value = value.clamp(props.value_min, props.value_max);
        let value = props.value;
        if let Some(widget) = self.widgets.get_mut(&id) {
            let metrics = Metrics::default();
            let ctx = IntentCtx {
                part: HitPart::Body,
                bounds: widget.bounds(),
                metrics: &metrics,
                phase: WidgetPhase::FrameEnd,
            };
            widget.on_intent(&Intent::Value(value), &ctx);
        }
        true
    }

    /// 由声明重建 dyn 表：**按 `WidgetId` 升序**，与插入 / 分配顺序无关。
    pub fn rebuild(&mut self, decls: &[WidgetDecl]) -> Result<(), String> {
        let mut sorted: Vec<&WidgetDecl> = decls.iter().collect();
        sorted.sort_by_key(|decl| decl.id);
        let mut widgets: BTreeMap<WidgetId, Box<dyn Widget2D>> = BTreeMap::new();
        let mut props_by_id: BTreeMap<WidgetId, WidgetProps> = BTreeMap::new();
        for decl in sorted {
            let Some(factory) = self.factories.get(&decl.spec) else {
                return Err(format!("未注册的控件规格：{}", decl.spec));
            };
            let props = WidgetProps {
                spec: decl.spec.clone(),
                label: decl.label.clone(),
                role: decl.role.clone(),
                value: decl.value,
                value_min: decl.value_min,
                value_max: decl.value_max,
                glyphs: self.glyphs.get(&decl.id).cloned().unwrap_or_default(),
            };
            widgets.insert(decl.id, factory(&props));
            props_by_id.insert(decl.id, props);
        }
        self.widgets = widgets;
        self.props = props_by_id;
        Ok(())
    }

    pub fn get(&self, id: WidgetId) -> Option<&dyn Widget2D> {
        self.widgets.get(&id).map(|widget| &**widget)
    }

    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut (dyn Widget2D + 'static)> {
        self.widgets.get_mut(&id).map(|widget| &mut **widget)
    }

    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.widgets.keys().copied()
    }

    /// 按稳定序测量全部控件（回溯）。
    pub fn measure_all(&self, ctx: &MeasureCtx<'_>) -> Vec<(WidgetId, Size)> {
        self.widgets
            .iter()
            .map(|(&id, widget)| (id, widget.measure(ctx)))
            .collect()
    }

    /// 按稳定序绘制（命令列表；派生态）。`hover` 只改颜色。
    pub fn draw_all(
        &self,
        bounds: &BTreeMap<WidgetId, Rect>,
        ctx: &DrawCtx<'_>,
        hover: Option<WidgetId>,
    ) -> Vec<DrawCommand> {
        let mut out = Vec::new();
        for (&id, widget) in &self.widgets {
            let Some(rect) = bounds.get(&id).copied() else {
                continue;
            };
            let child = DrawCtx {
                bounds: rect,
                clip: ctx.clip,
                theme_id: ctx.theme_id,
                themes: ctx.themes,
                metrics: ctx.metrics,
                env: ctx.env,
                z_base: ctx.z_base,
                hover: hover == Some(id),
                phase: ctx.phase,
            };
            out.extend(widget.draw(&child));
        }
        out
    }
}

/// 命令列表的**纯值摘要**（`z` + 变体名 + **几何** + style）。
///
/// CL 缺口 ②：此前只哈希 `z` / 变体名 / 颜色，两条 bounds 不同的 `Rect` 会得到同一摘要，
/// 「逐位相同」的判据检不出几何错位。现在几何按 `to_bits()` 逐分量进哈希。
pub fn command_digest(commands: &[DrawCommand]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for command in commands {
        for byte in command.z.0.to_le_bytes() {
            eat(byte);
        }
        for byte in command.shape.kind_name().bytes() {
            eat(byte);
        }
        digest_geometry(&command.shape, &mut eat);
        if let Some(color) = command.style.fill {
            for byte in [color.r(), color.g(), color.b(), color.a()] {
                eat(byte);
            }
        }
        if let Some(color) = command.style.stroke {
            for byte in [color.r(), color.g(), color.b(), color.a()] {
                eat(byte);
            }
        }
        for byte in command.style.stroke_width.to_bits().to_le_bytes() {
            eat(byte);
        }
    }
    hash
}

/// 几何入摘要：包围盒 + 各变体的关键参数（全部 `f32::to_bits()`）。
fn digest_geometry(shape: &Shape2D, eat: &mut impl FnMut(u8)) {
    let mut eat_f32 = |value: f32| {
        for byte in value.to_bits().to_le_bytes() {
            eat(byte);
        }
    };
    if let Some(bounds) = shape.bounds() {
        for value in [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y] {
            eat_f32(value);
        }
    }
    match shape {
        Shape2D::Line { start, end } => {
            for value in [start.x, start.y, end.x, end.y] {
                eat_f32(value);
            }
        }
        Shape2D::Polyline { points } | Shape2D::Polygon { points } => {
            eat_f32(points.len() as f32);
            for point in points {
                eat_f32(point.x);
                eat_f32(point.y);
            }
        }
        Shape2D::Circle { center, radius } => {
            for value in [center.x, center.y, *radius] {
                eat_f32(value);
            }
        }
        Shape2D::Ellipse { center, rx, ry } => {
            for value in [center.x, center.y, *rx, *ry] {
                eat_f32(value);
            }
        }
        _ => {}
    }
}

/// 命令包围盒（用于 `⊆ ctx.clip` 判据）。
pub fn command_bounds(commands: &[DrawCommand]) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for command in commands {
        let Some(bounds) = shape_bounds(&command.shape) else {
            continue;
        };
        out = Some(match out {
            None => bounds,
            Some(current) => Rect::new(
                Point::new(
                    current.min.x.min(bounds.min.x),
                    current.min.y.min(bounds.min.y),
                ),
                Point::new(
                    current.max.x.max(bounds.max.x),
                    current.max.y.max(bounds.max.y),
                ),
            ),
        });
    }
    out
}

fn shape_bounds(shape: &Shape2D) -> Option<Rect> {
    match shape {
        Shape2D::Rect(rect) => Some(Rect::new(
            Point::new(f64::from(rect.min.x), f64::from(rect.min.y)),
            Point::new(f64::from(rect.max.x), f64::from(rect.max.y)),
        )),
        Shape2D::Circle { center, radius } => Some(Rect::new(
            Point::new(f64::from(center.x - radius), f64::from(center.y - radius)),
            Point::new(f64::from(center.x + radius), f64::from(center.y + radius)),
        )),
        Shape2D::Polyline { points } | Shape2D::Polygon { points } => {
            let mut bounds: Option<Rect> = None;
            for point in points {
                let p = Point::new(f64::from(point.x), f64::from(point.y));
                bounds = Some(match bounds {
                    None => Rect::new(p, p),
                    Some(current) => Rect::new(
                        Point::new(current.min.x.min(p.x), current.min.y.min(p.y)),
                        Point::new(current.max.x.max(p.x), current.max.y.max(p.y)),
                    ),
                });
            }
            bounds
        }
        _ => None,
    }
}

/// 默认控件实现：一条轨道 + 可选圆钮 + 可选字形（`slider` / `button` / `label` / `radio`）。
pub struct BuiltinWidget {
    props: WidgetProps,
    bounds: Rect,
}

impl BuiltinWidget {
    pub fn new(props: WidgetProps) -> Self {
        Self {
            props,
            bounds: Rect::default(),
        }
    }
}

impl Widget2D for BuiltinWidget {
    fn measure(&self, ctx: &MeasureCtx<'_>) -> Size {
        // 固有高度由**控件种类**决定（与 `ControlKind::height` 逐位一致）；
        // 宽度铺满可用空间（`LayoutTokens.row_height` 只作缺省控件的高度兜底）。
        let height = self.kind().height();
        Size::new(
            ctx.constraints.max.width,
            if height > 0.0 {
                height
            } else {
                ctx.layout.row_height.max(1.0)
            },
        )
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn state(&self) -> WidgetState {
        WidgetState {
            role: self.props.role.clone(),
            name: self.props.label.clone(),
            value: Some(format!("{}", self.props.value)),
            focusable: true,
        }
    }

    /// 文本层：每个字形轮廓的包围盒就是一个字符格（标量下标 = 标签字符序）。
    fn text_run(&self) -> Option<crate::TextRun> {
        if self.props.glyphs.is_empty() {
            return None;
        }
        let mut run = crate::TextRun::new(self.bounds.height().max(1.0));
        let chars: Vec<char> = self.props.label.chars().collect();
        for (index, glyph) in self.props.glyphs.iter().enumerate() {
            let Some(ch) = chars.get(index).copied() else {
                break;
            };
            let Some(cell) = glyph.cell() else {
                continue;
            };
            run.push(index, ch, cell.min, cell.width().max(0.0));
        }
        Some(run)
    }

    fn draw(&self, ctx: &DrawCtx<'_>) -> Vec<DrawCommand> {
        let theme = ctx.theme();
        let track = if ctx.hover {
            theme.paint.hover
        } else {
            theme.paint.track
        };
        let mut commands = vec![DrawCommand::new(
            ctx.z_base,
            Shape2D::Rect(world_rect(ctx.metrics.to_world(ctx.bounds))),
            Style2D {
                fill: Some(color(track)),
                ..Style2D::default()
            },
        )];
        let t = if (self.props.value_max - self.props.value_min).abs() < f64::EPSILON {
            0.0
        } else {
            ((self.props.value - self.props.value_min)
                / (self.props.value_max - self.props.value_min))
                .clamp(0.0, 1.0)
        } as f32;
        // **按 SpecId 分派**（不再看 `role` 字符串）：只有滑块类画圆钮。
        if self.kind().has_knob() {
            let center = world_point(
                ctx.metrics,
                Point::new(
                    ctx.bounds.min.x + ctx.bounds.width() * f64::from(t),
                    ctx.bounds.center().y,
                ),
            );
            commands.push(DrawCommand::new(
                ZOrder(ctx.z_base.0 + 20),
                Shape2D::Circle {
                    center,
                    radius: (theme.layout.knob_radius * ctx.metrics.scale) as f32,
                },
                Style2D {
                    fill: Some(color(theme.paint.knob)),
                    ..Style2D::default()
                },
            ));
        }
        for outline in &self.props.glyphs {
            commands.push(DrawCommand::new(
                ZOrder(ctx.z_base.0 + 10),
                Shape2D::Path(outline.path.transformed(&ctx.metrics.to_transform())),
                Style2D {
                    fill: Some(color(theme.paint.label)),
                    ..Style2D::default()
                },
            ));
        }
        commands
    }

    /// 控件自处理意图（按 `SpecId` + `HitPart`，不再 `parse(name)`）：
    /// - `Pointer`：可交互控件消费（标签不消费 ⇒ 冒泡给容器）；
    /// - `Value`：滑块 / 单选组写回自己的 props 并消费。
    fn on_intent(&mut self, intent: &Intent, ctx: &IntentCtx<'_>) -> Verdict {
        match intent {
            Intent::Pointer(_) => {
                if self.kind().interactive() {
                    Verdict::Consume
                } else {
                    Verdict::Continue
                }
            }
            Intent::Value(value) => {
                if self.kind().has_knob() {
                    self.props.value = value.clamp(self.props.value_min, self.props.value_max);
                    let _ = ctx;
                    Verdict::Consume
                } else {
                    Verdict::Continue
                }
            }
            Intent::Activate => {
                if self.kind().interactive() {
                    Verdict::Consume
                } else {
                    Verdict::Continue
                }
            }
        }
    }
}

/// 内置控件的种类（**从 `SpecId` 派生**，不是从 `role` 字符串）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltinKind {
    Slider,
    Radio,
    Button,
    Label,
}

impl BuiltinKind {
    fn of(spec: &WidgetSpecId) -> Self {
        match spec.0.as_str() {
            "slider" => Self::Slider,
            "radio" => Self::Radio,
            "button" => Self::Button,
            _ => Self::Label,
        }
    }

    fn has_knob(self) -> bool {
        matches!(self, Self::Slider | Self::Radio)
    }

    /// 是否响应指针（标签只是文本）。
    fn interactive(self) -> bool {
        !matches!(self, Self::Label)
    }

    /// 固有高度（布局单位）。**必须与 `ControlKind::height` 逐位相同** ——
    /// 否则 `ControlSet` 的命中矩形与 `ui_layout` 的 `Frame` 会错位
    /// （用例 `builtin_heights_match_control_kind_heights` 守着这条）。
    fn height(self) -> f64 {
        match self {
            Self::Slider | Self::Radio => 20.0,
            Self::Button => 24.0,
            Self::Label => 16.0,
        }
    }
}

impl BuiltinWidget {
    fn kind(&self) -> BuiltinKind {
        BuiltinKind::of(&self.props.spec)
    }
}

fn world_point(metrics: &Metrics, point: Point) -> ze_notion::Vec2 {
    ze_notion::Vec2::new(
        (metrics.origin.x + point.x * metrics.scale) as f32,
        (metrics.origin.y + point.y * metrics.scale) as f32,
    )
}

fn world_rect(rect: Rect) -> ze_notion::Rect2D {
    ze_notion::Rect2D::new(
        ze_notion::Vec2::new(rect.min.x as f32, rect.min.y as f32),
        ze_notion::Vec2::new(rect.max.x as f32, rect.max.y as f32),
    )
}

fn color(rgba: [u8; 4]) -> ze_notion::Color32 {
    ze_notion::Color32::new(rgba[0], rgba[1], rgba[2], rgba[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(order: &[u32]) -> Vec<WidgetDecl> {
        order
            .iter()
            .map(|&id| WidgetDecl {
                id: LayoutId(id),
                spec: WidgetSpecId::new("slider"),
                parent: None,
                label: format!("row{id}"),
                role: "slider".to_string(),
                value: 0.5,
                ..WidgetDecl::default()
            })
            .collect()
    }

    fn runtime() -> WidgetRuntime {
        let mut runtime = WidgetRuntime::new();
        runtime.register("slider", |props| {
            Box::new(BuiltinWidget::new(props.clone()))
        });
        runtime
    }

    /// 绘制上下文：默认主题 + 恒等换算（测试只看布局单位）。
    fn draw_ctx<'a>(
        theme_id: &'a ThemeId,
        themes: &'a ThemeTable,
        metrics: &'a Metrics,
        env: &'a DrawEnv,
        clip: Option<Rect>,
    ) -> DrawCtx<'a> {
        DrawCtx {
            bounds: Rect::default(),
            clip,
            theme_id,
            themes,
            metrics,
            env,
            z_base: ZOrder(100),
            hover: false,
            phase: WidgetPhase::FrameEnd,
        }
    }

    fn default_theme_id() -> ThemeId {
        Theme::default_theme().id
    }

    /// **判据 6 / 12**：不同插入顺序重建 ⇒ 命令列表逐位相同。
    #[test]
    fn rebuild_is_insertion_order_independent() {
        let mut forward = runtime();
        forward.rebuild(&decls(&[0, 1, 2])).unwrap();
        let mut reversed = runtime();
        reversed.rebuild(&decls(&[2, 0, 1])).unwrap();
        assert_eq!(
            forward.ids().collect::<Vec<_>>(),
            reversed.ids().collect::<Vec<_>>()
        );

        let themes = ThemeTable::with_default();
        let metrics = Metrics::default();
        let env = DrawEnv::default();
        let theme_id = default_theme_id();
        let ctx = draw_ctx(&theme_id, &themes, &metrics, &env, None);
        let bounds: BTreeMap<WidgetId, Rect> = (0..3)
            .map(|id| {
                (
                    LayoutId(id),
                    Rect::from_min_size(Point::new(0.0, id as f64 * 20.0), 100.0, 18.0),
                )
            })
            .collect();
        assert_eq!(
            command_digest(&forward.draw_all(&bounds, &ctx, None)),
            command_digest(&reversed.draw_all(&bounds, &ctx, None))
        );
    }

    /// **判据 12**：`command_digest` 能区分几何（CL 缺口 ②）。
    #[test]
    fn command_digest_distinguishes_geometry() {
        let style = Style2D {
            fill: Some(color([10, 20, 30, 255])),
            ..Style2D::default()
        };
        let left = DrawCommand::new(
            ZOrder(1),
            Shape2D::Rect(ze_notion::Rect2D::new(
                ze_notion::Vec2::new(0.0, 0.0),
                ze_notion::Vec2::new(10.0, 10.0),
            )),
            style.clone(),
        );
        let right = DrawCommand::new(
            ZOrder(1),
            Shape2D::Rect(ze_notion::Rect2D::new(
                ze_notion::Vec2::new(5.0, 0.0),
                ze_notion::Vec2::new(15.0, 10.0),
            )),
            style.clone(),
        );
        assert_ne!(
            command_digest(&[left.clone()]),
            command_digest(&[right.clone()]),
            "bounds 不同的 Rect 必须摘要不同"
        );
        assert_eq!(
            command_digest(&[left.clone()]),
            command_digest(&[left]),
            "同一条命令两次摘要必须相同"
        );
    }

    /// **判据 7**：paint token 只改命令；layout token 改 revision。
    #[test]
    fn paint_tokens_change_commands_but_not_layout_revision() {
        let theme = Theme::default_theme();
        let mut repainted_theme = theme.clone();
        repainted_theme.paint.track = [1, 2, 3, 255];
        assert_eq!(
            theme.layout.revision(),
            repainted_theme.layout.revision(),
            "paint 不进 layout revision"
        );

        let mut themes = ThemeTable::with_default();
        themes.insert(theme.clone());
        let mut repainted = ThemeTable::with_default();
        repainted.insert(repainted_theme.clone());

        let runtime = {
            let mut runtime = runtime();
            runtime.rebuild(&decls(&[0])).unwrap();
            runtime
        };
        let env = DrawEnv::default();
        let metrics = Metrics::default();
        let theme_id = default_theme_id();
        let bounds: BTreeMap<WidgetId, Rect> = [(
            LayoutId(0),
            Rect::from_min_size(Point::new(0.0, 0.0), 100.0, 18.0),
        )]
        .into_iter()
        .collect();
        let draw = |themes: &ThemeTable| {
            let ctx = draw_ctx(&theme_id, themes, &metrics, &env, None);
            command_digest(&runtime.draw_all(&bounds, &ctx, None))
        };
        assert_ne!(draw(&themes), draw(&repainted), "换 paint token ⇒ 命令变");

        let mut relaid = theme.clone();
        relaid.layout.row_height = 30.0;
        assert_ne!(theme.layout.revision(), relaid.layout.revision());
    }

    /// **判据 8 / 12**：命令包围盒 ⊆ clip。
    #[test]
    fn command_bounds_stay_inside_the_clip() {
        let mut runtime = runtime();
        runtime.rebuild(&decls(&[0])).unwrap();
        let themes = ThemeTable::with_default();
        let metrics = Metrics::default();
        let env = DrawEnv::default();
        let bounds: BTreeMap<WidgetId, Rect> = [(
            LayoutId(0),
            Rect::from_min_size(Point::new(10.0, 10.0), 80.0, 18.0),
        )]
        .into_iter()
        .collect();
        let clip = Rect::from_min_size(Point::new(0.0, 0.0), 200.0, 200.0);
        let theme_id = default_theme_id();
        let ctx = draw_ctx(&theme_id, &themes, &metrics, &env, Some(clip));
        let commands = runtime.draw_all(&bounds, &ctx, None);
        let boxed = command_bounds(&commands).expect("有几何");
        assert!(boxed.min.x >= clip.min.x && boxed.max.x <= clip.max.x);
        assert!(boxed.min.y >= clip.min.y && boxed.max.y <= clip.max.y);
    }

    /// **判据 12**：世界换算进入命令（host 只给 metrics，不自己搬点）。
    #[test]
    fn metrics_map_layout_units_to_world() {
        // 用 `label`（无圆钮）⇒ 包围盒就是轨道矩形，不受旋钮半径干扰。
        let mut runtime = WidgetRuntime::new();
        runtime.register("label", |props| Box::new(BuiltinWidget::new(props.clone())));
        runtime
            .rebuild(&[WidgetDecl {
                id: LayoutId(0),
                spec: WidgetSpecId::new("label"),
                ..WidgetDecl::default()
            }])
            .unwrap();
        let themes = ThemeTable::with_default();
        let env = DrawEnv::default();
        let bounds: BTreeMap<WidgetId, Rect> = [(
            LayoutId(0),
            Rect::from_min_size(Point::new(0.0, 0.0), 10.0, 10.0),
        )]
        .into_iter()
        .collect();
        let metrics = Metrics::new(2.0, Point::new(100.0, 50.0));
        let theme_id = default_theme_id();
        let ctx = draw_ctx(&theme_id, &themes, &metrics, &env, None);
        let commands = runtime.draw_all(&bounds, &ctx, None);
        let boxed = command_bounds(&commands).expect("有几何");
        assert_eq!(boxed.min, Point::new(100.0, 50.0));
        assert_eq!(boxed.max.x, 120.0);
    }

    /// 内置控件的固有高度必须与 `ControlKind::height` 逐位一致（命中和 `Frame` 同口径）。
    #[test]
    fn builtin_heights_match_control_kind_heights() {
        use crate::control::ControlKind;
        let cases = [
            (
                BuiltinKind::Slider,
                ControlKind::Slider { min: 0.0, max: 1.0 },
            ),
            (
                BuiltinKind::Radio,
                ControlKind::RadioGroup {
                    options: &["a", "b"],
                },
            ),
            (BuiltinKind::Button, ControlKind::Button),
            (BuiltinKind::Label, ControlKind::Label),
        ];
        for (builtin, control) in cases {
            assert_eq!(
                builtin.height(),
                control.height(),
                "{builtin:?} / {control:?}"
            );
        }
    }

    /// hover 只改轨道颜色、不改命令条数。
    #[test]
    fn hover_changes_only_colors() {
        let mut runtime = runtime();
        runtime.rebuild(&decls(&[0])).unwrap();
        let themes = ThemeTable::with_default();
        let metrics = Metrics::default();
        let env = DrawEnv::default();
        let theme_id = default_theme_id();
        let bounds: BTreeMap<WidgetId, Rect> = [(
            LayoutId(0),
            Rect::from_min_size(Point::new(0.0, 0.0), 100.0, 18.0),
        )]
        .into_iter()
        .collect();
        let ctx = draw_ctx(&theme_id, &themes, &metrics, &env, None);
        let normal = runtime.draw_all(&bounds, &ctx, None);
        let hovered = runtime.draw_all(&bounds, &ctx, Some(LayoutId(0)));
        assert_eq!(normal.len(), hovered.len(), "hover 不改命令条数");
        assert_ne!(
            normal[0].style.fill, hovered[0].style.fill,
            "hover 必须改轨道颜色"
        );
    }

    /// 未注册的规格必须显式报错（漏了会红）。
    #[test]
    fn unknown_spec_is_an_explicit_error() {
        let mut runtime = WidgetRuntime::new();
        let error = runtime
            .rebuild(&[WidgetDecl {
                spec: WidgetSpecId::new("missing"),
                ..WidgetDecl::default()
            }])
            .unwrap_err();
        assert!(error.contains("missing"));
    }
}
