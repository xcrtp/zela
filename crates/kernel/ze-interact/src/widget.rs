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

use crate::hit::Rect;
use crate::layout::Size;
use crate::layout::{Constraints, LayoutId};
use crate::Point;
use ze_render_ir::{DrawCommand, Shape2D, Style2D, ZOrder};

/// 控件在声明期的稳定 id（与 `Frame::geometry` 同口径；内容寻址，不是分配序）。
pub type WidgetId = LayoutId;

/// 控件规格（工厂表的键）。**独立命名空间**，不与 `ContentOriginKind` / `OpRef` 共用。
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

/// 布局策略（本轮只留形状；默认两遍扫描，kasuari 将来只加一个工厂项）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StrategyId(pub String);

impl StrategyId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// 主题 id（进 `Diagnostics.environment`，不进 payload）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ThemeId(pub String);

/// **布局 token**：会改变 measure 结果（因此必须进 revision）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutTokens {
    pub row_height: f64,
    pub gap: f64,
    pub padding: f64,
    pub font_size: f64,
    pub knob_radius: f64,
}

impl LayoutTokens {
    pub fn revision(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in [
            self.row_height,
            self.gap,
            self.padding,
            self.font_size,
            self.knob_radius,
        ] {
            for byte in value.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

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
#[derive(Debug, Clone, Default)]
pub struct ThemeTable {
    themes: BTreeMap<ThemeId, Theme>,
}

impl ThemeTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default() -> Self {
        let mut table = Self::default();
        table.insert(Theme::default_theme());
        table
    }

    pub fn insert(&mut self, theme: Theme) {
        self.themes.insert(theme.id.clone(), theme);
    }

    pub fn get(&self, id: &ThemeId) -> Option<&Theme> {
        self.themes.get(id)
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
    pub theme: &'a Theme,
    pub env: &'a DrawEnv,
    pub z_base: ZOrder,
    /// 只读相位标签，禁跨相位副作用。
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
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetDecl {
    pub id: WidgetId,
    pub spec: WidgetSpecId,
    pub parent: Option<WidgetId>,
    pub label: String,
    pub role: String,
    pub value: f64,
}

/// 一段已烘焙的字形轮廓（纯 f64 折线；不引入 `Path2D`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphOutline {
    pub points: Vec<Point>,
    pub closed: bool,
}

/// 工厂入参（纯值；控件自己的 props）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetProps {
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
    /// 控件自处理意图；默认继续冒泡。
    fn on_intent(&mut self, _intent: &Intent) -> Verdict {
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
#[derive(Default)]
pub struct WidgetRuntime {
    widgets: BTreeMap<WidgetId, Box<dyn Widget2D>>,
    factories: BTreeMap<WidgetSpecId, WidgetFactory>,
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

    /// 由声明重建 dyn 表：**按 `WidgetId` 升序**，与插入 / 分配顺序无关。
    pub fn rebuild(&mut self, decls: &[WidgetDecl]) -> Result<(), String> {
        let mut sorted: Vec<&WidgetDecl> = decls.iter().collect();
        sorted.sort_by_key(|decl| decl.id);
        let mut widgets: BTreeMap<WidgetId, Box<dyn Widget2D>> = BTreeMap::new();
        for decl in sorted {
            let Some(factory) = self.factories.get(&decl.spec) else {
                return Err(format!("未注册的控件规格：{}", decl.spec));
            };
            let props = WidgetProps {
                label: decl.label.clone(),
                role: decl.role.clone(),
                value: decl.value,
                value_min: 0.0,
                value_max: 1.0,
                glyphs: Vec::new(),
            };
            widgets.insert(decl.id, factory(&props));
        }
        self.widgets = widgets;
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

    /// 按稳定序绘制（命令列表；派生态）。
    pub fn draw_all(&self, bounds: &BTreeMap<WidgetId, Rect>, ctx: &DrawCtx<'_>) -> Vec<DrawCommand> {
        let mut out = Vec::new();
        for (&id, widget) in &self.widgets {
            let Some(rect) = bounds.get(&id).copied() else {
                continue;
            };
            let child = DrawCtx {
                bounds: rect,
                clip: ctx.clip,
                theme: ctx.theme,
                env: ctx.env,
                z_base: ctx.z_base,
                phase: ctx.phase,
            };
            out.extend(widget.draw(&child));
        }
        out
    }
}

/// 命令列表的**纯值摘要**（只比 `(z, shape 变体, style)`；`DrawCommand` 的 `Ord` 只比 z）。
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

    fn track_color(&self, theme: &Theme) -> [u8; 4] {
        theme.paint.track
    }
}

impl Widget2D for BuiltinWidget {
    fn measure(&self, ctx: &MeasureCtx<'_>) -> Size {
        Size::new(
            ctx.constraints.max.width,
            ctx.layout.row_height.max(1.0),
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
            let mut min = Point::new(f64::INFINITY, f64::INFINITY);
            let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
            for point in &glyph.points {
                min = Point::new(min.x.min(point.x), min.y.min(point.y));
                max = Point::new(max.x.max(point.x), max.y.max(point.y));
            }
            if !min.x.is_finite() {
                continue;
            }
            run.push(index, ch, min, (max.x - min.x).max(0.0));
        }
        Some(run)
    }

    fn draw(&self, ctx: &DrawCtx<'_>) -> Vec<DrawCommand> {
        let mut commands = vec![DrawCommand::new(
            ctx.z_base,
            Shape2D::Rect(rect_to_notion(ctx.bounds)),
            Style2D {
                fill: Some(color(self.track_color(ctx.theme))),
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
        if self.props.role == "slider" || self.props.role == "radio" {
            let center = ze_notion::Vec2::new(
                ctx.bounds.min.x as f32 + ctx.bounds.width() as f32 * t,
                ctx.bounds.center().y as f32,
            );
            commands.push(DrawCommand::new(
                ZOrder(ctx.z_base.0 + 20),
                Shape2D::Circle {
                    center,
                    radius: ctx.theme.layout.knob_radius as f32,
                },
                Style2D {
                    fill: Some(color(ctx.theme.paint.knob)),
                    ..Style2D::default()
                },
            ));
        }
        for outline in &self.props.glyphs {
            let points: Vec<ze_notion::Vec2> = outline
                .points
                .iter()
                .map(|p| ze_notion::Vec2::new(p.x as f32, p.y as f32))
                .collect();
            let shape = if outline.closed {
                Shape2D::Polygon { points }
            } else {
                Shape2D::Polyline { points }
            };
            commands.push(DrawCommand::new(
                ZOrder(ctx.z_base.0 + 10),
                shape,
                Style2D {
                    fill: Some(color(ctx.theme.paint.label)),
                    ..Style2D::default()
                },
            ));
        }
        commands
    }
}

fn rect_to_notion(rect: Rect) -> ze_notion::Rect2D {
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

    /// **判据 6**：不同插入 / 分配顺序重建 ⇒ 命令列表逐位相同。
    #[test]
    fn rebuild_is_insertion_order_independent() {
        let mut forward = runtime();
        forward.rebuild(&decls(&[0, 1, 2])).unwrap();
        let mut reversed = runtime();
        reversed.rebuild(&decls(&[2, 0, 1])).unwrap();
        assert_eq!(forward.ids().collect::<Vec<_>>(), reversed.ids().collect::<Vec<_>>());

        let theme = Theme::default_theme();
        let env = DrawEnv::default();
        let ctx = DrawCtx {
            bounds: Rect::default(),
            clip: None,
            theme: &theme,
            env: &env,
            z_base: ZOrder(100),
            phase: WidgetPhase::FrameEnd,
        };
        let bounds: BTreeMap<WidgetId, Rect> = (0..3)
            .map(|id| {
                (
                    LayoutId(id),
                    Rect::from_min_size(Point::new(0.0, id as f64 * 20.0), 100.0, 18.0),
                )
            })
            .collect();
        assert_eq!(
            command_digest(&forward.draw_all(&bounds, &ctx)),
            command_digest(&reversed.draw_all(&bounds, &ctx))
        );
    }

    /// **判据 7**：paint token 只改命令；layout token 改 revision。
    #[test]
    fn paint_tokens_change_commands_but_not_layout_revision() {
        let theme = Theme::default_theme();
        let mut repainted = theme.clone();
        repainted.paint.track = [1, 2, 3, 255];
        assert_eq!(
            theme.layout.revision(),
            repainted.layout.revision(),
            "paint 不进 layout revision"
        );

        let runtime = {
            let mut runtime = runtime();
            runtime.rebuild(&decls(&[0])).unwrap();
            runtime
        };
        let env = DrawEnv::default();
        let bounds: BTreeMap<WidgetId, Rect> = [(
            LayoutId(0),
            Rect::from_min_size(Point::new(0.0, 0.0), 100.0, 18.0),
        )]
        .into_iter()
        .collect();
        let draw = |theme: &Theme| {
            let ctx = DrawCtx {
                bounds: Rect::default(),
                clip: None,
                theme,
                env: &env,
                z_base: ZOrder(100),
                phase: WidgetPhase::FrameEnd,
            };
            command_digest(&runtime.draw_all(&bounds, &ctx))
        };
        assert_ne!(draw(&theme), draw(&repainted), "换 paint token ⇒ 命令变");

        let mut relaid = theme.clone();
        relaid.layout.row_height = 30.0;
        assert_ne!(theme.layout.revision(), relaid.layout.revision());
    }

    /// **判据 8**：命令包围盒 ⊆ clip。
    #[test]
    fn command_bounds_stay_inside_the_clip() {
        let mut runtime = runtime();
        runtime.rebuild(&decls(&[0])).unwrap();
        let theme = Theme::default_theme();
        let env = DrawEnv::default();
        let bounds: BTreeMap<WidgetId, Rect> = [(
            LayoutId(0),
            Rect::from_min_size(Point::new(10.0, 10.0), 80.0, 18.0),
        )]
        .into_iter()
        .collect();
        let clip = Rect::from_min_size(Point::new(0.0, 0.0), 200.0, 200.0);
        let ctx = DrawCtx {
            bounds: Rect::default(),
            clip: Some(clip),
            theme: &theme,
            env: &env,
            z_base: ZOrder(100),
            phase: WidgetPhase::FrameEnd,
        };
        let commands = runtime.draw_all(&bounds, &ctx);
        let boxed = command_bounds(&commands).expect("有几何");
        assert!(boxed.min.x >= clip.min.x && boxed.max.x <= clip.max.x);
        assert!(boxed.min.y >= clip.min.y && boxed.max.y <= clip.max.y);
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
