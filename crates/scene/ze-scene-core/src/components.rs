// components.rs
//
// 场景组件集合（文件模块）：mesh / 3D 核心类型，
// 以及 Circle/Path/Line/PhaseAnim 等纯逻辑 2D 组件。
// 渲染相关样式组件（StrokeSolid/FillSolid）在 zela-render 中。
//
// B3-1 收尾（H-12）：原 `marker::{Device, WGPUDevice, CPUDevice}` 已删除 ——
// 后端设备概念不该编码在场景组件里（D7），且全仓无消费方。
//
// 注：旧动画组件（ClockState / LocalClock / CircleAnim / OrbitAnim）已删除，
// 动画统一由 Binding 表 + ze-time::Curve 纯函数驱动。
//
// M1 T4：连续数学对象（FunctionX2Y / ParameterCurveT2XY / ParameterCurveT2XYZ /
// Sampler1D / Sampler2D / Interpolator / DynFn）已归位理念层 `ze-notion`
// （func / curve / sample），此处**不再 re-export** —— 消费方直接改 import。
//
// M3-5：图元组件（Circle / Path / Line）已迁往**场景对象层** `ze-object`
// （newtype 封装理念层 `Circle2D` / `Path2D` / `Line2D`，消掉"字段两处定义"）。
// 本层（ECS 实现）只消费，不再定义；`Transform` / `Parent` / `MorphShape` 等
// 仍在下方（它们的归位见交付说明的延后清单）。

pub mod mesh {
    use ze_notion::Vec3;

    pub struct TriangleMesh {
        pub vertices: Vec<Vec3>,
        pub indices: Vec<u32>,
    }
    pub struct QuadMesh {
        pub vertices: Vec<Vec3>,
        pub indices: Vec<u32>,
    }
    #[derive(Debug, Clone)]
    pub struct PathMesh {
        pub vertices: Vec<Vec3>,
        pub indices: Vec<u32>,
    }
}

/// 3D 核心类型（PrimitiveTopology / MeshGeometry / Material3D /
/// Projection / Camera3D）。
/// 文件名以数字开头，无法直接作为 Rust 标识符，通过 `#[path]` 挂载为 `three_d`。
#[path = "components/3d.rs"]
pub mod three_d;

pub use mesh::*;
pub use three_d::*;

/// FunctionX2Y 相位滑动参数：`phase = t * omega`，
/// 由 `SceneRecordSystem` 采样时取 `x + phase` 实现曲线水平平移。
/// `omega` 为常量；`phase` 由 `BindingApplySystem` 每帧覆盖。
#[derive(
    Debug,
    Clone,
    derive_builder::Builder,
    derive_new::new,
    ze_scene_macros::Channels,
    serde::Serialize,
    serde::Deserialize,
)]
// F 批：phase 用持久化名覆盖（既有 payload 兼容）；omega 是常量不注册。
#[channel(name = "Phase", phase: f32)]
pub struct PhaseAnim {
    pub phase: f32,
    pub omega: f32,
}

/// 可变形形状：持有 `from` / `to` 与目标参数 `t`，由 [`crate::animation::MorphApplySystem`]
/// 每帧插值进同实体的 [`Path`]（render 层由 `record_scene` 消费 `(Path, StrokeSolid)`）。
///
/// 设计（S1 计划 §3.3）：**只写 `t`**（一个 f32），不每帧重建 `from`/`to` —— 为 S3
/// 上 GPU 预留（from/to 上传一次，t 每帧一个 float）。描边样式（`StrokeSolid`）
/// 由 zela-render 提供，作为同一实体上的独立组件，本组件只管几何。
#[derive(Debug, Clone, ze_scene_macros::Channels, serde::Serialize, serde::Deserialize)]
// F 批：只注册 `t`（设计上唯一每帧变量，见类型文档）；from/to/samples 不是通道。
#[channel(name = "MorphT", t: f32)]
pub struct MorphShape {
    pub from: ze_notion::Path2D,
    pub to: ze_notion::Path2D,
    /// 形变参数 ∈ [0, 1]，由 `BindingApplySystem`（`BindingTarget::MorphT`）写入。
    pub t: f32,
    /// 输出顶点数（恒定；与 from/to 复杂度无关）。
    pub samples: u32,
}

impl MorphShape {
    /// 用默认顶点数（[`ze_notion::DEFAULT_MORPH_SAMPLES`]）构造，`t = 0`（= from）。
    pub fn new(from: ze_notion::Path2D, to: ze_notion::Path2D) -> Self {
        Self {
            from,
            to,
            t: 0.0,
            samples: ze_notion::DEFAULT_MORPH_SAMPLES as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ContentTextureItem;

/// **书写动画（reveal）对象**：持有一条描边路径 + 可见区间 `[start, end]`（归一化弧长）。
///
/// 与 [`MorphShape`] 同级、同一设计口径（几何静态、变量走 uniform）：
/// - `path` 帧间不变 ⇒ 后端按内容哈希缓存三角化与 GPU 缓冲（稳态零上传）；
/// - `start` / `end` 由 [`crate::animation::target::RevealRange`] 每帧写入
///   （`AnimValue::Scalar` → `end`；`AnimValue::Vec2` → `(start, end)`），
///   走 2D uniform 的 `params.z / params.w`。
///
/// 为什么是**独立组件**（而不是给 [`Path`](crate::scene::components) 加个区间字段）：
/// `(Path, StrokeSolid)` 会被 `SceneRecordSystem` 录进 `DrawQueue` **整条绘制**，
/// 而 reveal 对象必须**只**由 reveal 通路绘制（否则会叠加一条完整路径）。
/// 本组件不在 `record_scene` 的组件组合枚举里 ⇒ 天然互斥。
///
/// 默认 `start = 0` / `end = 1` = **完整可见**（与不带 reveal 的绘制等价）。
#[derive(Debug, Clone, ze_scene_macros::Channels, serde::Serialize, serde::Deserialize)]
// F 批：start / end 全量注册（end 缺省名 = 规范地址 "Reveal.end"）；path 不是通道。
#[channel(start: f32)]
#[channel(end: f32)]
pub struct Reveal {
    pub path: ze_notion::Path2D,
    /// 可见区间起点（归一化弧长 ∈ [0,1]），由 `RevealRange` 写入。
    pub start: f32,
    /// 可见区间终点（归一化弧长 ∈ [0,1]），由 `RevealRange` 写入。
    pub end: f32,
}

impl Reveal {
    /// 构造：`[0, 0]`（**尚未写出任何部分** —— 书写动画的自然起点）。
    pub fn new(path: ze_notion::Path2D) -> Self {
        Self {
            path,
            start: 0.0,
            end: 0.0,
        }
    }

    /// 构造：完整可见（`[0, 1]`）。
    pub fn visible(path: ze_notion::Path2D) -> Self {
        Self {
            path,
            start: 0.0,
            end: 1.0,
        }
    }

    /// 书写**笔尖**位置：归一化弧长 `end` 处的点（派生量，不落字段）。
    ///
    /// 口径与渲染侧裁剪（`clip_reveal`）一致：路径先 `flatten(0.25)`，再按**累计弦长**
    /// 定位；`end ≤ 0` ⇒ 起点，`end ≥ 1` ⇒ 终点。这是 `Reveal.current_pos`
    /// 只读通道的实现，也是"小球跟随笔尖"能走读端管道的前提。
    ///
    /// 弧长逻辑只有一份：[`ze_notion::Path2D::point_at`]（`tolerance = 0.25` 为默认口径，
    /// 与 `Reveal.point_at` 方法同源 ⇒ 两处采样不可能分叉）。
    pub fn current_pos(&self) -> ze_notion::Vec2 {
        self.path.point_at(self.end, REVEAL_TOLERANCE)
    }
}

/// reveal / 路径跟随共用的**展平容差**（写成常量：两侧口径一致是契约，不是巧合）。
pub const REVEAL_TOLERANCE: f32 = 0.25;

/// **路径跟随**：对象沿一条折线按归一化弧长 `t` 移动（`orient` 时同时转向）。
///
/// 与 [`Reveal`] 同一口径（`Path2D::from_polyline` + 同一展平容差），但**不绘制**：
/// 它只提供"路径 + 参数"这一对数据，位置由
/// `PathFollowApplySystem`（ECS 层，SceneSync）每帧写进同实体的 `Transform`。
///
/// 为什么不是"给 `Transform` 加个字段"：路径跟随是**派生**（位置 = f(path, t)），
/// 与 `MorphShape.t → Path` 同型；把 `path` 塞进摆放组件会让 Transform 不再可序列化
/// 成三个数，并破坏父子累乘的单一口径。
///
/// 与 [`MorphShape`] 的差别：那里 `t` 驱动几何插值（几何每帧变），这里 `t` 驱动
/// **摆放**（几何恒定、位置变）；因此本组件不产生 `Path`，也不参与绘制。
#[derive(Debug, Clone, ze_scene_macros::Channels, serde::Serialize, serde::Deserialize)]
// F 批：只注册 `t`（`path` / `orient` 是静态数据，不是每帧变量）。
#[channel(t: f32)]
pub struct PathFollow {
    /// 被跟随的路径（装配期由折线点表构造，帧间不变）。
    pub path: ze_notion::Path2D,
    /// 归一化弧长 ∈ [0, 1]，由 `BindingApplySystem`（地址 `PathFollow.t`）写入。
    pub t: f32,
    /// `true` ⇒ 同时把切线方向写进 `Transform.rotation`；`false` ⇒ 只写位置。
    pub orient: bool,
}

impl PathFollow {
    /// 构造（`t = 0`，不转向）——`orient` 由脚本显式给，不在构造器里猜。
    pub fn new(path: ze_notion::Path2D) -> Self {
        Self {
            path,
            t: 0.0,
            orient: false,
        }
    }

    /// `t` 处的位置（与 [`Reveal::current_pos`] 同一份弧长实现、同一容差）。
    pub fn position_at(&self, t: f32) -> ze_notion::Vec2 {
        self.path.point_at(t, REVEAL_TOLERANCE)
    }

    /// `t` 处的**朝向角**（绕 Z，弧度 = `atan2(tangent.y, tangent.x)`）。
    pub fn angle_at(&self, t: f32) -> f32 {
        let tangent = self.path.tangent_at(t, REVEAL_TOLERANCE);
        tangent.y.atan2(tangent.x)
    }
}

// M3-4：`Transform`（摆放组件）与 `Parent`（关系）已迁出本层 ——
// 前者归场景对象层 `ze-object`，后者统一用契约层
// `ze_scene_contract::Parent(pub ObjectId)`。原场景层的 `Parent` 组件把父对象
// 存成 ECS 实体句柄，是"场景层绑死具体 ECS"的最后一处组件形态，已删除。

macro_rules! define_comp {
    ($name:ident, $ty:ty) => {
        #[derive(
            derive_more::Deref, derive_more::DerefMut, derive_more::From, derive_more::Into,
        )]
        struct $name($ty);
    };
}

define_comp!(Position2, ze_notion::Vec2);
define_comp!(Position3, ze_notion::Vec3);
