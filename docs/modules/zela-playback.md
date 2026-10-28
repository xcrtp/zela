# zela-playback

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`app`（`crates/app`）
- 路径：`crates/app/zela-playback`
- 生产依赖（上游，dev-deps 不计）：`ze-notion`、`ze-pod-core`、`ze-pod-func`、`ze-render-ir`、`ze-scene-core`、`zela-render`、`zela-render-backends`、`zela-render-vector`、`zela-scene-bundle`、`zela-scene-ecs`、`zela-schedule`
- 被依赖（下游）：`zela-app`、`zela-gui`、`zela-web`
- 源文件：4 个 · 2728 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/input_replay.rs` | 99 | **输入流重放**（D 批）：把「第 N 帧的事件」装进 `InputEvents`。 |
| `src/lib.rs` | 581 | `zela-playback` — **回放运行时**（浏览器与 native `--bundle` 共用）。 |
| `src/vector3d.rs` | 825 | B10 **3D 场景 → 矢量后端**的桥：`Camera3D` → [`View3`]、`LightQueue` → 灯、 |
| `src/vector_frame.rs` | 1223 | 一帧内容的**矢量渲染**与路由特征（B11 Step 4/5）。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/input_replay.rs` | `pub fn apply_input_frame(driver: &DriverPod, events: &[SceneEvent])` |
| `src/lib.rs` | `pub fn init_tracing()` |
| `src/lib.rs` | `pub enum PlaybackError` |
| `src/lib.rs` | `pub struct Runtime` |
| `src/lib.rs` | `pub struct SceneAssembly` |
| `src/lib.rs` | `pub fn available_playback_caps() -> Available` |
| `src/lib.rs` | `pub fn build_bundle_runtime(render_ctx: RenderContext, bundle: &Bundle) -> Result<Runtime>` |
| `src/lib.rs` | `pub fn bundle_render_params(bundle: &Bundle) -> Result<zela_scene_bundle::RenderParams>` |
| `src/lib.rs` | `pub fn check_render_params_supported(` |
| `src/lib.rs` | `pub fn assemble_runtime(render_ctx: RenderContext, assembly: SceneAssembly) -> Runtime` |
| `src/lib.rs` | `pub struct ViewportControl` |
| `src/lib.rs` | `pub fn viewport_control(runtime: &Runtime) -> ViewportControl` |
| `src/lib.rs` | `pub fn install_camera3d(runtime: &Runtime)` |
| `src/lib.rs` | `pub fn apply_viewport_control(runtime: &Runtime)` |
| `src/lib.rs` | `pub fn find_camera2d(scene: &Scene) -> Result<Option<Camera2DHandle>>` |
| `src/lib.rs` | `pub fn set_view_rect(runtime: &Runtime, width: f32, height: f32)` |
| `src/lib.rs` | `pub fn step_frame(runtime: &Runtime, dt: f32) -> Option<SceneCompositorCallback>` |
| `src/lib.rs` | `pub fn end_frame(runtime: &Runtime)` |
| `src/lib.rs` | `pub fn advance_and_render(` |
| `src/lib.rs` | `pub fn render_step_to_view(` |
| `src/lib.rs` | `pub fn vector_capabilities() -> ze_render_ir::Capabilities` |
| `src/lib.rs` | `pub fn vector_params(width: u32, height: u32) -> zela_scene_bundle::RenderParams` |
| `src/lib.rs` | `pub fn to_rgba(ctx: &RenderContext, pixels: &[u8]) -> Vec<u8>` |
| `src/lib.rs` | `pub fn rgba_to_target(ctx: &RenderContext, rgba: &[u8]) -> Vec<u8>` |
| `src/vector3d.rs` | `pub fn set_shadows(on: bool)` |
| `src/vector3d.rs` | `pub fn shadows() -> bool` |
| `src/vector3d.rs` | `pub fn set_environment(on: bool)` |
| `src/vector3d.rs` | `pub fn environment() -> bool` |
| `src/vector3d.rs` | `pub fn environment_light(params: &RenderParams) -> Option<Light>` |
| `src/vector3d.rs` | `pub fn view3(` |
| `src/vector3d.rs` | `pub fn lights(queue: Option<&LightQueue>) -> Vec<Light>` |
| `src/vector3d.rs` | `pub fn mesh_records(queue: &DrawQueue, lines: LineStyle) -> Result<Vec<PathRecord>, String>` |
| `src/vector3d.rs` | `pub struct LineStyle` |
| `src/vector3d.rs` | `pub fn has_meshes(queue: &DrawQueue) -> bool` |
| `src/vector_frame.rs` | `pub struct VectorFrame` |
<!-- AUTO:END -->

## 职责

回放运行时：把 ZSB 产物还原成场景、推进调度并出帧，浏览器与 native 的 --bundle 路径共用这一份。

## 运行机制

入口是由产物建运行时的函数：可还原性检查 → 渲染参数校验 → 还原本场景脚本 → commit 进场景 → 交给装配尾（全仓唯一一份，脚本路径也走它）。产物是运行时结构（命令驱动推进器 + 渲染上下文 + 时长），宿主随后用视口设置、单帧推进、推进并渲染、帧结束几个入口逐帧驱动。3D 桥把相机与光照、网格队列转成矢量后端的视图与路径记录，矢量帧模块负责一帧内容的渲染与路由特征，输入重放按帧号把录制的事件装回输入队列。

## 边界与不做什么

- 不依赖语言适配器与脚本运行时 —— 正因如此才能进浏览器。
- 不编译脚本、不编码产物：那由编译入口与产物格式 crate 负责。
- 不建窗口、不碰 DOM：GUI 与浏览器壳分别是 zela-gui 与 zela-web。
