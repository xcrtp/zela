# ze-scene-api

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`scene`（`crates/scene`）
- 路径：`crates/scene/ze-scene-api`
- 生产依赖（上游，dev-deps 不计）：`ze-content`、`ze-notion`、`ze-pod-core`、`ze-pod-macro`、`ze-prerender`、`ze-render-ir`、`ze-scene-contract`、`ze-scene-core`、`ze-scene-macros`、`ze-time`
- 被依赖（下游）：`zela-scene-ecs`
- 源文件：5 个 · 1922 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/api.rs` | 197 | — |
| `src/content_service.rs` | 202 | — |
| `src/lib.rs` | 87 | 脚本 Pod 的 **domain 侧**：能力定义 + `scene` / `content` 两个 Service。 |
| `src/scene_service.rs` | 910 | — |
| `src/types.rs` | 526 | — |

### 模块内关系（`use crate::…` / `use super::…`）

```mermaid
graph LR
    f1["src/content_service.rs"]
    f0["src/scene_service.rs"]
    f0 --> f1
```

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/api.rs` | `pub struct SceneApi<'a>` |
| `src/api.rs` | `pub struct ContentApi<'a>` |
| `src/content_service.rs` | `pub struct PathStore(Arc<Mutex<PathStoreInner>>)` |
| `src/content_service.rs` | `pub struct ContentServicePod` |
| `src/lib.rs` | `pub trait SceneComponent: ze_pod_core::PodType` |
| `src/lib.rs` | `pub trait RegisterSceneComponent` |
| `src/lib.rs` | `pub fn scene_service(paths: PathStore) -> Arc<SceneServicePod>` |
| `src/lib.rs` | `pub fn content_service(` |
| `src/scene_service.rs` | `pub struct SceneServicePod` |
| `src/scene_service.rs` | `pub fn channels() -> &'static [ze_pod_core::ChannelDecl]` |
| `src/scene_service.rs` | `pub fn methods() -> &'static [ze_pod_core::MethodDecl]` |
| `src/types.rs` | `pub struct Color` |
| `src/types.rs` | `pub struct Vec2` |
| `src/types.rs` | `pub struct Vec3` |
| `src/types.rs` | `pub struct Vec4` |
| `src/types.rs` | `pub struct Camera3D` |
| `src/types.rs` | `pub struct Cube` |
| `src/types.rs` | `pub struct Transform3D` |
| `src/types.rs` | `pub enum ShaderVariant` |
| `src/types.rs` | `pub struct Material3D` |
| `src/types.rs` | `pub enum Texture` |
| `src/types.rs` | `pub enum SurfaceMapping` |
| `src/types.rs` | `pub enum SurfaceRenderer` |
| `src/types.rs` | `pub enum TextureFilter` |
| `src/types.rs` | `pub struct Surface` |
| `src/types.rs` | `pub enum LightKind` |
| `src/types.rs` | `pub struct Light` |
| `src/types.rs` | `pub struct Circle` |
| `src/types.rs` | `pub struct Stroke` |
| `src/types.rs` | `pub struct Camera2D` |
| `src/types.rs` | `pub struct Reveal` |
| `src/types.rs` | `pub struct Polyline` |
| `src/types.rs` | `pub struct Parent` |
| `src/types.rs` | `pub struct Name` |
| `src/types.rs` | `pub struct Opacity` |
| `src/types.rs` | `pub struct Group` |
| `src/types.rs` | `pub struct PhaseAnim` |
| `src/types.rs` | `pub struct Morph` |
| `src/types.rs` | `pub struct LineStyle` |
| `src/types.rs` | `pub struct Axes2D` |
| `src/types.rs` | `pub struct Arrow` |
| `src/types.rs` | `pub struct Fill` |
| `src/types.rs` | `pub struct Gradient` |
| `src/types.rs` | `pub struct Dash` |
| `src/types.rs` | `pub struct SecondPaint` |
| `src/types.rs` | `pub struct BackgroundStroke` |
| `src/types.rs` | `pub struct CurveWindow` |
| `src/types.rs` | `pub struct Transform` |
| `src/types.rs` | `pub struct Path` |
| `src/types.rs` | `pub struct PathList(pub Vec<Path>)` |
| `src/types.rs` | `pub struct RevealPath` |
| `src/types.rs` | `pub struct PathShape` |
| `src/types.rs` | `pub struct Formula` |
| `src/types.rs` | `pub struct Text` |
| `src/types.rs` | `pub struct ScalarKey` |
| `src/types.rs` | `pub enum KeyframeInterp` |
<!-- AUTO:END -->

## 职责

`ze-scene-api` 是脚本 Pod 的 **domain 侧**：能力定义 + `scene` / `content` 两个 Service。它持有 `SceneApi`（16 项）/ `ContentApi`（2 项）的 capability 声明、PodType 结构体参数表，以及两个 Service 的运行时状态 —— `SceneServicePod` 的组件 / 对象句柄表与录制日志、`ContentServicePod` 的 `PathStore`。

## 运行机制

脚本调用 → 绑定层按 `TypeSpec` 把参数转成本 crate 的 `types` 结构 → `SceneServicePod` 把结构化值落成场景原语，按组件组展开（`RevealPath` = `Reveal` + `StrokeSolid`）写进 `SceneEncoder`；内部交接函数 `__finish` 取走录制结果并重置，`__reset` 丢弃上一轮状态。内容侧走另一条路：`ContentServicePod` 把源码交给 `ze-prerender` 的 `InMemoryEngine` 编译，产出的描边路径按阅读顺序存进 `PathStore`（句柄 owner 是这张表自己的 `PodId`，两个 Service 都只是它的用户），再以句柄交给场景。`SceneComponent` + `RegisterSceneComponent` 让 Pod 类型名与构造器名同源，一条注册不再手写三个字符串。

内部文件分工：`api.rs` 是能力定义（`#[capability]` 生成 `calls()`），`types.rs` 是全部结构化参数 / 返回值的 PodType schema，`scene_service.rs` 是场景 Service 与录制状态，`content_service.rs` 是内容 Service 与 `PathStore`，`lib.rs` 是两个注册 trait 与两个构造入口。

## 边界与不做什么

- **零 hecs、零 `zela-*`**（`JQ_SCENE_LAYERS` 判据守住）：不能反向依赖 ECS 内部或任何实现层 crate。
- **不认识任何语言运行时**：没有 mlua / pyo3；语言适配器只依赖这里的能力类型，绑定驱动在 `ze-pod-binding`，与本 crate 无编译期耦合。
- **不做场景逻辑**：对象 / 组件 / 动画的语义在 `ze-scene-core`，这里只做「结构化值 → 场景原语」的落点转换。
- **不做排版**：内容编译由 `ze-prerender` / `ze-content` 完成，本 crate 只做引擎调用与类型适配；也不依赖 `zela-render`，否则 scene 层会反向依赖 render 层。
- **不定义绘制参数真源**：`Fill` / `Stroke` 等在这里只是脚本面结构体，落点是 `ze-render-ir` 的 `FillSolid` / `StrokeSolid`。
