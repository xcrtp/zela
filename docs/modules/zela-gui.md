# zela-gui

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`app`（`crates/app`）
- 路径：`crates/app/zela-gui`
- 生产依赖（上游，dev-deps 不计）：`ze-pod-core`、`zela-app`、`zela-egui`、`zela-playback`、`zela-render-backends`、`zela-render-vector`、`zela-scene-ecs`、`zela-schedule`
- 被依赖（下游）：（无）
- 源文件：2 个 · 954 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 872 | — |
| `src/main.rs` | 82 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub enum Initial` |
| `src/lib.rs` | `pub struct App` |
<!-- AUTO:END -->

## 职责

GUI 可执行壳：把组装层与 egui 前端撮合成一个 eframe 应用。

## 运行机制

主函数先初始化结构化日志，再按启动参数（指定作品 id、走脚本、走 Python 分镜、走能力演示）解析出初始选中项，交给 eframe 启动。应用壳持有当前运行时，顶部下拉可在内置作品与用户打开的脚本间切换 —— 切换等于整体重新装配运行时；后端可切 Auto / 矢量 / Wgpu，路由复用渲染层的同一实现。产物是桌面可执行程序，矢量帧的呈现走离屏读回再贴成纹理，导出与对拍仍走 headless。

## 边界与不做什么

- 不承担装配与渲染实现：运行时来自组装与回放层，帧体来自 egui 前端。
- 不是唯一入口：headless 渲染、编译、对拍各有独立二进制。
- 不做运行态场景增删：换作品就是重新装配。
