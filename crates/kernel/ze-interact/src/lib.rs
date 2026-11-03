//! `ze-interact` — 交互骨架（命中 / 布局 / 路由）。
//!
//! # 它回答三个问题
//!
//! 1. **点到了谁** —— [`hit`]：矩形命中 + 线性索引（按 z-order 降序）；
//! 2. **东西摆在哪** —— [`layout`]：盒子 / 流式布局（纯函数）；
//! 3. **事件发给谁** —— [`route`]：hover / focus / capture 三档。
//!
//! # 边界（写死，避免长成 grab bag）
//!
//! - **不绘制**：UI 只是场景对象（线 / 圆 / 由 typst 烘焙的文字轮廓）；
//! - **不认 egui**：宿主负责把原生输入翻译成语义事件；
//! - **不做控件**（按钮 / 滑块 / 面板）：那是应用层的组合；
//! - **不做文字排版**：只留度量接口形状，实现属后续批次；
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
//! **零依赖**（纯 `core` / `std`）：不依赖 `glam`、不依赖 `ze-notion`。
//! 曲线求交（贝塞尔）只留 [`hit::CurveHitTester`] 接口 —— 现有实现在场景层
//! 依赖 glam，直接搬进来会破坏本 crate 的零依赖约束。

#![forbid(unsafe_code)]

pub mod hit;
pub mod layout;
pub mod route;

pub use hit::{CurveHitTester, HitId, HitIndex, HitRegion, HitTarget, LinearHitIndex, Point, Rect};
pub use layout::{
    apply_layout, BoxLayout, Constraints, FlowLayout, Layout, LayoutId, LayoutItem, PlacedRect,
    Size,
};
pub use route::{RouteKey, RoutePhase, Router};
