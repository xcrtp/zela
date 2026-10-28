# zela-web

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`web`（`crates/web`）
- 路径：`crates/web/zela-web`
- 生产依赖（上游，dev-deps 不计）：`ze-render-ir`、`zela-playback`、`zela-render-backends`、`zela-render-vector`、`zela-scene-bundle`
- 被依赖（下游）：（无）
- 源文件：1 个 · 1052 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 1052 | `zela-web` — 浏览器薄壳（**唯一**允许碰 DOM / WebGPU 初始化的地方）。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn adapter_facts() -> String` |
| `src/lib.rs` | `pub fn set_backend(kind: &str) -> Result<(), JsValue>` |
| `src/lib.rs` | `pub fn set_speed(speed: f32)` |
<!-- AUTO:END -->

## 职责

浏览器薄壳：全仓唯一允许碰 DOM 与 WebGPU 初始化的地方，负责取产物、起循环、出帧。

## 运行机制

启动入口按 URL 参数取 .zsb，体积超限在解析前就拒绝，然后解出产物并交给回放层建运行时；随后由 requestAnimationFrame 驱动，时间步固定为 1/30，调速只改变每个回调推进的仿真帧数。显示按 canvas 表面格式走，对拍另开离屏纹理加异步回读，用帧序列哈希给结论。播放代次计数器让切后端变成「重播」而不是整页重载，同一时刻最多一条循环在画。

## 边界与不做什么

- 只编 wasm32：整个 crate 以目标架构条件收口，不参与 native 构建。
- 不放场景与渲染语义，全部委托回放层与渲染后端。
- 不编译脚本、不提供编辑器界面。
