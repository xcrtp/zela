//! `ze-interact` — 交互骨架（命中 / 布局 / 路由 / 帧）。
//!
//! # 它回答四个问题
//!
//! 1. **点到了谁** —— [`hit`]：矩形命中 + 线性索引（按 z-order 降序）；
//! 2. **东西摆在哪** —— [`layout`]：盒子 / 流式布局（纯函数）；
//! 3. **事件发给谁** —— [`route`]：hover / focus / capture 三档；
//! 4. **这一帧长什么样** —— [`frame`]：几何 / 语义 / 文本三条平行结构的快照；
//! 5. **控件怎么描述** —— [`control`]：`ControlSpec` → 事件状态机 → 几何 + 语义。
//!
//! # 边界（写死，避免长成 grab bag）
//!
//! - **不绘制**：UI 只是场景对象（线 / 圆 / 由轮廓烘焙的文字）；
//! - **不认 egui**：宿主负责把原生输入翻译成语义事件；
//! - **不做控件外观**：控件只描述"摆在哪、叫什么、值多少、点下去发生什么"；
//! - **不做文字排版**：度量在 `ze-content` / `ze-prerender`；
//!   [`text::TextRun`] 只承载「字符索引 ↔ 矩形」这份结果；
//! - **不算深度模型**：只做「谁在前面就先命中谁」，**不解决 2D–3D 互遮挡**。
//!
//! # 精度口径（C1）
//!
//! 输入来自渲染空间（f32），本 crate **内部一律 f64**，输出在边界降回 f32。
//! 与 `ze-alg` 的 f64 口径同源：中间不反复降级。
//!
//! # 确定性（C2）
//!
//! 命中结果按 z-order **降序**、同 z 按插入顺序；不使用 `HashMap` / `HashSet`。
//! 路由状态（hover / focus / capture）**不进产物**，靠重放事件流重建。
//!
//! # 依赖
//!
//! **默认零依赖**（纯 `core` / `std`）：不依赖 `glam`、不依赖 `ze-notion`。
//! `serde` feature 只为 [`frame::Frame`] 的导出（`--features serde`）。
//! `widgets` feature（CL-b）打开表达层：`draw` 输出 `DrawCommand`，会间接引入
//! `ze-notion` / `glam` —— 这是显式选择的代价，默认构建不受影响。
//! 曲线求交（贝塞尔）只留 [`hit::CurveHitTester`] 接口 —— 现有实现在场景层
//! 依赖 glam，直接搬进来会破坏本 crate 的默认零依赖约束。

#![forbid(unsafe_code)]

pub mod control;
pub mod frame;
pub mod hit;
pub mod layout;
pub mod route;
pub mod text;
/// **CL-b**：表达层（`Widget2D` / `Container2D` / `WidgetRuntime` / ctx / theme）。
/// 需要 `widgets` feature（`DrawCommand` 会间接引入 glam；默认构建不受影响）。
#[cfg(feature = "widgets")]
pub mod widget;

pub use control::{
    ControlEvent, ControlKind, ControlOutcome, ControlSet, ControlSpec, constraints_for,
};
pub use frame::{Frame, SemanticNode};
pub use hit::{
    CurveHitTester, HitChain, HitFlow, HitId, HitIndex, HitRegion, HitTarget, LinearHitIndex, Point,
    Rect,
};
pub use layout::{
    apply_layout, BoxLayout, ConstraintSet, Constraints, FlowLayout, Layout, LayoutCache, LayoutId,
    LayoutItem, PlacedRect, Size, layout_for_constraints,
};
pub use route::{RouteKey, RoutePhase, Router};
pub use text::{CharBox, TextRun, TextRunMarker};
