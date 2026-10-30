# zela-app

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`app`（`crates/app`）
- 路径：`crates/app/zela-app`
- 生产依赖（上游，dev-deps 不计）：`ze-comb`、`ze-content`、`ze-macro`、`ze-notion`、`ze-pod-binding`、`ze-pod-core`、`ze-pod-func`、`ze-pod-js`、`ze-pod-lua`、`ze-pod-py`、`ze-prerender`、`ze-render-ir`、`ze-scene-core`、`zela-authored`、`zela-playback`、`zela-render`、`zela-render-backends`、`zela-render-vector`、`zela-scene-bundle`、`zela-scene-ecs`、`zela-schedule`
- 被依赖（下游）：`zela-cli`、`zela-commands`、`zela-gui`
- 源文件：10 个 · 8012 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/assembly.rs` | 1000 | — |
| `src/bin/zela-parity.rs` | 788 | `zela-parity`：**作品级**双后端对拍（B13 Step 1/2）。 |
| `src/bin/zela-render.rs` | 687 | `zela-render`：headless 薄壳 —— 脚本直接渲到视频。 |
| `src/bin/zela-verify.rs` | 719 | `zela-verify`：**批量**对拍入口（B9 Step 3）。 |
| `src/demos.rs` | 145 | 内置作品清单（GUI 的"作品"下拉；默认加载**全部** demo）。 |
| `src/export.rs` | 467 | — |
| `src/formula.rs` | 585 | — |
| `src/frame_loop.rs` | 739 | 帧循环（B9 Step 3）：`zela-render` 与 `zela-verify` 共用的**唯一**渲染循环。 |
| `src/lib.rs` | 2741 | — |
| `src/python_env.rs` | 141 | 内嵌 Python 的**前置探测**（`feature = "python"`）。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/assembly.rs` | `pub fn plan(with_python: bool, with_js: bool) -> AssemblyPlan` |
| `src/assembly.rs` | `pub enum AssemblyError` |
| `src/assembly.rs` | `pub struct PodAssembly` |
| `src/assembly.rs` | `pub fn assemble(engine: Arc<InMemoryEngine>) -> Result<PodAssembly, AssemblyError>` |
| `src/demos.rs` | `pub struct Demo` |
| `src/demos.rs` | `pub fn demo(id: &str) -> Option<&'static Demo>` |
| `src/export.rs` | `pub struct ExportSpec` |
| `src/export.rs` | `pub enum ExportError` |
| `src/export.rs` | `pub struct ExportReport` |
| `src/export.rs` | `pub fn export_sequence<F>(` |
| `src/export.rs` | `pub fn target_pix_fmt(ctx: &RenderContext) -> Result<&'static str, ExportError>` |
| `src/export.rs` | `pub fn to_rgba(ctx: &RenderContext, pixels: &[u8]) -> Vec<u8>` |
| `src/export.rs` | `pub fn rgba_to_target(ctx: &RenderContext, rgba: &[u8]) -> Vec<u8>` |
| `src/export.rs` | `pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), ExportError>` |
| `src/formula.rs` | `pub struct FormulaSpec` |
| `src/formula.rs` | `pub fn build_engine() -> InMemoryEngine` |
| `src/formula.rs` | `pub fn rebuild(engine: &InMemoryEngine, spec: &FormulaSpec) -> Option<ContentIR>` |
| `src/formula.rs` | `pub struct TextSpec` |
| `src/formula.rs` | `pub fn compile_to_stroke_paths(ir: &ContentIR, spec: &FormulaSpec) -> Vec<StrokePath>` |
| `src/formula.rs` | `pub fn compile_morph_pairs(` |
| `src/frame_loop.rs` | `pub struct FrameLoopOptions` |
| `src/frame_loop.rs` | `pub struct FrameView<'a>` |
| `src/frame_loop.rs` | `pub struct FrameTiming` |
| `src/frame_loop.rs` | `pub struct FrameStats` |
| `src/frame_loop.rs` | `pub fn timing_json(t: &FrameTiming, frame: u32) -> String` |
| `src/frame_loop.rs` | `pub fn summary_json(t: &FrameTiming, window: &str, frames: usize) -> String` |
| `src/frame_loop.rs` | `pub fn summary_body(t: &FrameTiming, window: &str, frames: usize) -> String` |
| `src/frame_loop.rs` | `pub fn environment_json(adapter: Option<&wgpu::Adapter>) -> String` |
| `src/frame_loop.rs` | `pub fn accumulate(` |
| `src/frame_loop.rs` | `pub fn frames_for(runtime: &Runtime, fps: f64) -> u32` |
| `src/frame_loop.rs` | `pub struct FrameRun` |
| `src/frame_loop.rs` | `pub fn run_frames(` |
| `src/lib.rs` | `pub enum SceneChoice` |
| `src/lib.rs` | `pub enum ScriptLanguage` |
| `src/lib.rs` | `pub fn language_from_path(path: &std::path::Path) -> Result<ScriptLanguage>` |
| `src/lib.rs` | `pub fn build_script_runtime(` |
| `src/lib.rs` | `pub struct CompiledScript` |
| `src/lib.rs` | `pub fn compile_script(source: &str, language: ScriptLanguage) -> Result<CompiledScript>` |
| `src/lib.rs` | `pub fn available_runtime_caps() -> zela_scene_bundle::Available` |
| `src/lib.rs` | `pub fn build_bundle_runtime(` |
| `src/lib.rs` | `pub fn build_scene_runtime(render_ctx: RenderContext, choice: SceneChoice) -> Result<Runtime>` |
| `src/python_env.rs` | `pub fn ensure() -> Result<(), String>` |
<!-- AUTO:END -->

## 职责

组装层：做 Pod 静态装配、建资源与系统图、构建 GUI 与 headless 共用的运行时，并给出 headless 的三个可执行入口。

## 运行机制

库侧入口是几个函数：脚本与内置场景各自建好场景后交给回放 crate 的装配尾收尾，脚本编译入口供 zela-compile 产出产物，帧循环则是渲染与批量对拍两个二进制共用的唯一循环。装配分两层：清单函数只产出纯数据的节点描述，装配函数才建 domain 服务与语言适配器，交给 Broker 校验、排序与注入。产物按二进制分：渲染入口输出 PNG 序列或经 ffmpeg 管道出视频，批量对拍入口输出逐帧哈希，作品级对拍入口输出双后端比较报告；导出不做 seek，只能边跑边抓像素。

## 边界与不做什么

- 不依赖 zela-egui，也不建 GUI 壳：那是 zela-gui。
- 不写第二套帧循环或第二份渲染路径：所有入口共用同一帧循环与同一份运行时装配。
- 不持有场景内容：内置作品的摆放与时序在 zela-authored。
