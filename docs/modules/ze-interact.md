# ze-interact

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`kernel`（`crates/kernel`）
- 路径：`crates/kernel/ze-interact`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`ze-content`、`ze-prerender`、`zela-app`、`zela-authored`、`zela-node-editor`、`zela-scene-ecs`
- 源文件：7 个 · 1655 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/control.rs` | 423 | **CG-d**：控件层 —— 从「画控件」变成「描述控件」。 |
| `src/frame.rs` | 188 | **CG-c**：结构化帧（`Frame`）—— UI 的一次**快照**。 |
| `src/hit.rs` | 241 | 命中测试（CE-a）：矩形优先 + 索引接口预留。 |
| `src/layout.rs` | 438 | 布局（CE-b）：盒子 + 流式，**纯函数、无求解器**。 |
| `src/lib.rs` | 56 | `ze-interact` — 交互骨架（命中 / 布局 / 路由 / 帧）。 |
| `src/route.rs` | 205 | 路由（CE-c）：hover / focus / capture 三档。 |
| `src/text.rs` | 104 | **CG-b/c**：文本的逐字符盒（字符下标 ↔ 矩形）。 |

### 模块内关系（`use crate::…` / `use super::…`）

```mermaid
graph LR
    f0["src/frame.rs"]
    f1["src/hit.rs"]
    f2["src/layout.rs"]
    f3["src/text.rs"]
    f0 --> f1
    f0 --> f2
    f0 --> f3
```

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/control.rs` | `pub enum ControlKind` |
| `src/control.rs` | `pub struct ControlSpec` |
| `src/control.rs` | `pub enum ControlEvent` |
| `src/control.rs` | `pub enum ControlOutcome` |
| `src/control.rs` | `pub struct ControlSet` |
| `src/control.rs` | `pub fn constraints_for(specs: &[ControlSpec], max_width: f64) -> Constraints` |
| `src/frame.rs` | `pub struct Frame` |
| `src/frame.rs` | `pub struct SemanticNode` |
| `src/hit.rs` | `pub struct Point` |
| `src/hit.rs` | `pub struct Rect` |
| `src/hit.rs` | `pub struct HitId(pub u32)` |
| `src/hit.rs` | `pub enum HitRegion` |
| `src/hit.rs` | `pub struct HitTarget` |
| `src/hit.rs` | `pub trait HitIndex` |
| `src/hit.rs` | `pub struct LinearHitIndex` |
| `src/hit.rs` | `pub trait CurveHitTester` |
| `src/layout.rs` | `pub struct LayoutId(pub u32)` |
| `src/layout.rs` | `pub struct Size` |
| `src/layout.rs` | `pub struct Constraints` |
| `src/layout.rs` | `pub struct LayoutItem` |
| `src/layout.rs` | `pub struct PlacedRect` |
| `src/layout.rs` | `pub trait Layout` |
| `src/layout.rs` | `pub struct BoxLayout` |
| `src/layout.rs` | `pub struct FlowLayout` |
| `src/layout.rs` | `pub fn apply_layout(` |
| `src/layout.rs` | `pub struct ConstraintSet` |
| `src/layout.rs` | `pub fn layout_for_constraints(` |
| `src/layout.rs` | `pub struct LayoutCache` |
| `src/route.rs` | `pub struct RouteKey` |
| `src/route.rs` | `pub enum RoutePhase` |
| `src/route.rs` | `pub struct Router` |
| `src/text.rs` | `pub struct CharBox` |
| `src/text.rs` | `pub struct TextRun` |
<!-- AUTO:END -->

## 职责

kernel 级的交互骨架：只回答三个问题 —— **点到了谁**（矩形命中 + 线性索引）、**东西摆在哪**（盒子 / 流式布局）、**事件发给谁**（hover / focus / capture 路由）。它不是控件库：不绘制、不认 egui、不做按钮 / 滑块 / 面板，也不做文字排版。

## 运行机制

命中测试把场景坐标（内部 f64）与轴对齐矩形做半开区间比较，`LinearHitIndex` 按 z-order 降序返回（同 z 保持插入顺序，稳定排序）。布局是纯函数：`BoxLayout` 垂直堆叠、`FlowLayout` 水平换行，`apply_layout` 统一处理「自动布局不得覆盖用户手选位置」（`auto_layout_supported() == false` 时原样返回既有位置）。路由是一个无时间、无随机来源的状态机：指针移动更新 hover，按下建立 focus 与 capture，抬起只解除 capture；事件投递按 capture > focus > hover 取一个。路由键 = 作用域路径 + 命中目标，因此两个面板的同号控件可以区分。

## 边界与不做什么

- **不绘制**：UI 只是场景对象（线 / 圆 / 由 typst 烘焙的文字轮廓）。
- **不认 egui**：宿主把原生输入翻译成语义事件，本 crate 只消费坐标与 id。
- **不做控件**：按钮 / 滑块 / 面板是应用层的组合。
- **不做文字排版**：只留度量接口形状，实现属后续批次。
- **不算深度模型**：只做「谁在前面就先命中谁」，不解决 2D–3D 互遮挡。
- **零 glam / 零 HashMap / 零依赖**：曲线求交只留 `CurveHitTester` 接口（现有贝塞尔实现依赖 glam，不搬进来）。
- 布局只在**结构变化时**算（控件增删 / 文本变化），不在每帧调用。
