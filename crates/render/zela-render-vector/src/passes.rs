// SPDX-FileCopyrightText: 2026 Academa, Inc.
// SPDX-License-Identifier: MIT
// Modified by Zela: BN 批新建 —— pass 声明、资源槽位与调度校验（不参与着色算法）。

//! pass 声明化与资源槽位（BN 批）。
//!
//! # 本模块只做三件事
//!
//! 1. **资源名 → binding 槽位**：按声明序分配，消掉手写编号；
//! 2. **pass 顺序变数据**：`PassPlan` 是一张有序声明表，调度按它走；
//! 3. **只校验不推导**：用 `ze_graph_core::find_cycle` 查环、查"生产者先于消费者"，
//!    **不做自动拓扑排序** —— 就绪集合按 id 取最小会重排无依赖边的 pass，
//!    而顺序一旦不确定，同一声明集两次编译就可能出不同画面。
//!
//! # 为什么"无依赖边保持声明序"要写成契约
//!
//! `ze_graph_core::topo_sort` 的就绪集合用 `BTreeSet` 取最小 id（那是它的确定性承诺，
//! 不是 bug）。本模块**不使用**它来决定顺序：顺序就是声明的下标。用例
//! [`tests::undeclared_order_is_declaration_order`] 固定这一点。
//!
//! # 与像素的关系
//!
//! 本模块全是纯数据与纯函数，不碰 wgpu、不碰字节：改坏它只会让校验报错，
//! 不会让画面变化。真正的"变了"由 `--step hashes` 判定。

use ze_graph_core::{Adjacency, find_cycle};
// BP-2：组的名字表（纯数据，定义在 kernel 层 IR）—— 名字 → 组号 → 它携带的可声明资源名。
use ze_render_ir::{BindSymbols, GroupSymbol};

// ─── 资源槽位：组 1（vector images） ──────────────────────────────────────

/// 组 1 一个槽位的绑定种类（决定 `BindGroupLayoutEntry` 怎么建）。
///
/// 与 `vector.rs` 里原来手写的那 24 个 entry 一一对应，只是把"编号"换成"声明序"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// 可被合成阶段采样的普通纹理（不可过滤、单采样）。
    Texture,
    /// 计算阶段采样、可多采样（`msaa` 由实例决定）。
    SampledCompute { multisampled: bool, depth: bool },
    /// 深度阵列（阴影图）。
    DepthArray,
    /// 可过滤纹理（DFG 表）。
    FilterableTexture,
    /// 立方体纹理（环境）。
    Cube,
    /// 不可过滤的浮点阵列（slabs）。
    FloatArray,
    /// 只写存储纹理：视图颜色（`super::COLOR`）。
    StorageColor,
    /// 只写存储纹理：灯光（`super::RADIANCE`）。
    StorageRadiance,
    /// 读存储缓冲。
    BufferRead,
    /// 读存储缓冲（同一 `kind` 族里的读写态，合成用的 `crossings`）。
    BufferReadWrite,
    /// 普通采样器。
    Sampler,
    /// 比较采样器。
    ComparisonSampler,
}

/// 一个组 1 槽位：名字 + 绑定种类。**数组下标即 binding 编号**。
#[derive(Debug, Clone, Copy)]
pub struct ImageSlot {
    pub name: &'static str,
    pub kind: ImageKind,
}

impl ImageSlot {
    const fn new(name: &'static str, kind: ImageKind) -> Self {
        Self { name, kind }
    }
}

/// 组 1 的全部槽位，顺序 = binding 编号（0..23）。
///
/// # 必须是稠密且只追加的
///
/// - **稠密**：编号来自数组下标，中间留空会让后面的槽位整体偏移；
/// - **只追加**：在中间插入会让其后所有槽位的编号变化 ⇒ 必须同步改 `vector.wgsl`
///   的 `@group(1) @binding(n)`。用例 [`tests::image_slots_are_pinned`] 逐项固定当前编号。
///
/// 名字取自 shader（`vector.wgsl` / `vector_buffers.wgsl` / `vector_compute.wgsl`），
/// 不是新造的词。
pub const IMAGE_SLOTS: [ImageSlot; 24] = [
    ImageSlot::new("atlas", ImageKind::Texture),
    ImageSlot::new("layers", ImageKind::Texture),
    ImageSlot::new("base", ImageKind::Texture),
    ImageSlot::new("view_out", ImageKind::StorageColor),
    ImageSlot::new("base_depth", ImageKind::Texture),
    ImageSlot::new("heads", ImageKind::BufferRead),
    ImageSlot::new("nodes", ImageKind::BufferRead),
    ImageSlot::new(
        "base_samples",
        ImageKind::SampledCompute {
            multisampled: true,
            depth: false,
        },
    ),
    ImageSlot::new(
        "base_sample_depths",
        ImageKind::SampledCompute {
            multisampled: true,
            depth: true,
        },
    ),
    ImageSlot::new("stroke_depths_in", ImageKind::BufferRead),
    ImageSlot::new("shadow_maps", ImageKind::DepthArray),
    ImageSlot::new("shadow_compare", ImageKind::ComparisonSampler),
    ImageSlot::new("dfg_table", ImageKind::FilterableTexture),
    ImageSlot::new("dfg_linear", ImageKind::Sampler),
    ImageSlot::new("crossings", ImageKind::BufferReadWrite),
    ImageSlot::new("environment", ImageKind::Cube),
    ImageSlot::new("occlusion", ImageKind::Texture),
    ImageSlot::new("base_light", ImageKind::Texture),
    ImageSlot::new(
        "base_light_samples",
        ImageKind::SampledCompute {
            multisampled: true,
            depth: false,
        },
    ),
    ImageSlot::new("light_out", ImageKind::StorageRadiance),
    ImageSlot::new("slab_bounds", ImageKind::Texture),
    ImageSlot::new("slabs", ImageKind::FloatArray),
    ImageSlot::new("shader_programs", ImageKind::BufferRead),
    ImageSlot::new("shader_colors", ImageKind::Texture),
];

/// 组 1 槽位总数（= 声明序分配的上界）。
pub const IMAGE_SLOT_COUNT: u32 = IMAGE_SLOTS.len() as u32;

/// 资源名 → 组 1 的 binding 编号。未登记的名字 **panic 并点名**（不静默给 0）。
pub fn image_slot(name: &str) -> u32 {
    IMAGE_SLOTS
        .iter()
        .position(|slot| slot.name == name)
        .unwrap_or_else(|| panic!("组 1 没有登记资源 {name:?}（IMAGE_SLOTS 缺项）")) as u32
}

// ─── 组名表（BP-2） ──────────────────────────────────────────────────────

/// `PassDecl.inputs` 里会用到的组 1（images）名字。
///
/// 每个都必须是 `IMAGE_SLOTS` 的槽位（用例 [`tests::groups_only_name_registered_slots`] 逐项比对），
/// 不是新造的词；这里只列"声明会引用"的那些，不是 layout 的副本。
const IMAGE_NAMES: [&str; 13] = [
    "atlas",
    "layers",
    "base",
    "base_depth",
    "crossings",
    "slab_bounds",
    "slabs",
    "occlusion",
    "environment",
    "shadow_maps",
    "dfg_table",
    "shader_programs",
    "shader_colors",
];

/// 光栅视图的组名表（`RASTER_PLAN` / `RASTER_LAYERED_PLAN`）。
///
/// `resources` 只列声明会引用的名字：`scene_group` 还绑着 `views` / `vertices` / `links` /
/// `extras`，但没有任何 `PassDecl.inputs` 引用它们。
///
/// 组 3（`lighting`）的清单为空：光照是每个受光 pass 都绑的环境组，声明侧目前不描述
/// 光照依赖 —— 空清单 ⇒ 不参与"多绑"判定（BP-4 记账）。
pub const RASTER_GROUPS: BindSymbols = BindSymbols::new(&[
    GroupSymbol {
        group: 0,
        name: "scene",
        resources: &["instances", "rows", "sprites"],
    },
    GroupSymbol {
        group: 2,
        name: "list",
        resources: &["list_appended"],
    },
    GroupSymbol {
        group: 3,
        name: "lighting",
        resources: &[],
    },
]);

/// 矢量合成视图的组名表（`VECTOR_PLAN`）。
///
/// 两个组都可能是组 0，但 layout 不同：光栅的 `scene` 带实例缓冲，矢量的
/// `vector scene` 带曲线控制点。名字分开记，免得"声明了 `ctrl` 却只绑了光栅组"被放过。
pub const VECTOR_GROUPS: BindSymbols = BindSymbols::new(&[
    GroupSymbol {
        group: 0,
        name: "vector scene",
        resources: &["ctrl", "subs", "objects", "rows", "tiles"],
    },
    GroupSymbol {
        group: 1,
        name: "images",
        resources: &IMAGE_NAMES,
    },
    GroupSymbol {
        group: 3,
        name: "lighting",
        resources: &[],
    },
]);

// ─── 目标纹理身份 ────────────────────────────────────────────────────────

/// 一帧内被写入的纹理目标。名字用于声明与校验，**不承担分配职责**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderTargetId {
    /// 视图颜色（离屏 target / 画布）。
    View,
    /// 视图灯光。
    ViewLight,
    /// 光栅基线（paths 的基色）。
    Base,
    /// 光栅基线灯光。
    BaseLight,
    /// 光栅基线的深度（单采样）。
    BaseDepth,
    /// 点 / 网格的层。
    Layers,
    /// 光栅图集。
    Atlas,
    /// 光栅图集的深度。
    AtlasDepth,
    /// 环境预滤波的分级。
    Environment,
    /// 泛光的分级。
    Bloom,
    /// 遮蔽的分级。
    Occlusion,
    /// 视图的遮蔽深度。
    OcclusionDepth,
    /// 阴影图的一层。
    ShadowLayer,
    /// 链表头。
    ListHeads,
    /// 链表节点。
    ListNodes,
    /// 链表追加计数。
    ListAppended,
    /// 遮蔽深度（自定义）。
    StrokeDepths,
}

impl RenderTargetId {
    /// 名字（错误信息与声明表用）。
    pub const fn name(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::ViewLight => "view_light",
            Self::Base => "base",
            Self::BaseLight => "base_light",
            Self::BaseDepth => "base_depth",
            Self::Layers => "layers",
            Self::Atlas => "atlas",
            Self::AtlasDepth => "atlas_depth",
            Self::Environment => "environment",
            Self::Bloom => "bloom",
            Self::Occlusion => "occlusion",
            Self::OcclusionDepth => "occlusion_depth",
            Self::ShadowLayer => "shadow_layer",
            Self::ListHeads => "list_heads",
            Self::ListNodes => "list_nodes",
            Self::ListAppended => "list_appended",
            Self::StrokeDepths => "stroke_depths",
        }
    }
}

// ─── pass 声明 ───────────────────────────────────────────────────────────

/// pass 的记录方式（决定框架怎么起 pass）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassStage {
    /// 渲染 pass（有颜色 / 深度附件）。
    Render,
    /// 计算 pass。
    Compute,
}

impl PassStage {
    const fn label(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Compute => "compute",
        }
    }
}

/// 条件跳过：`skip & mask == 0` 时执行。
///
/// 复用既有的 `SKIP_*` 位（`render.rs` 的 `skipping()`），**不另造一套开关**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnabledWhen(pub u32);

impl EnabledWhen {
    /// 无条件执行。
    pub const ALWAYS: Self = Self(0);

    /// 在 `skip` 位集下是否执行。
    pub const fn is_enabled(self, skip: u32) -> bool {
        skip & self.0 == 0
    }
}

/// 一个 pass 的声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassDecl {
    /// 唯一名字（运行时 recording 必须按这个名字报数）。
    pub name: &'static str,
    pub kind: PassStage,
    /// 读哪些逻辑资源（组 1 的槽位名，或上一 pass 的输出）。
    pub inputs: &'static [&'static str],
    /// 写哪些目标（`RenderTargetId::name()`）。
    pub outputs: &'static [&'static str],
    /// 顶点 / 片段 / 计算入口名（纯记录：入口是 shader 的事实，不在这里推导）。
    pub entry: &'static str,
    /// dispatch 形状（纯记录，供文档与后续 codegen 使用）。
    pub dispatch: Dispatch,
    pub enabled_when: EnabledWhen,
}

/// dispatch 形状（声明用）。BO 批起由 [`crate::dispatch`] 定义 —— 除数跟随
/// `@workgroup_size`，形状只描述被除数怎么算。
pub use crate::dispatch::{Dispatch, GroupSize};

/// 一帧的 pass 序列声明（顺序 = 执行顺序）。
pub type PassPlan = &'static [PassDecl];

/// 光栅视图、**无链表 / 无 slabs** 的 pass 序列。
pub const RASTER_PLAN: PassPlan = &[
    PassDecl {
        name: "shadow",
        kind: PassStage::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["shadow_layer"],
        entry: "shadow",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "occlusion depth",
        kind: PassStage::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["occlusion_depth"],
        entry: "occlusion depth",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "occlusion",
        kind: PassStage::Compute,
        inputs: &["occlusion_depth"],
        outputs: &["occlusion"],
        entry: "occlusion",
        dispatch: Dispatch::Tiles {
            group: GroupSize::Entry("sao"),
        },
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "base",
        kind: PassStage::Render,
        inputs: &["instances", "rows", "sprites"],
        outputs: &["view", "atlas_depth"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "base depth",
        kind: PassStage::Compute,
        inputs: &["base_samples", "base_sample_depths"],
        outputs: &["base_depth"],
        entry: "nearest_of_samples",
        dispatch: Dispatch::Tiles {
            group: GroupSize::Entry("nearest_of_samples"),
        },
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "vector composite",
        kind: PassStage::Compute,
        inputs: &["ctrl", "subs", "objects", "base", "atlas", "base_depth", "crossings"],
        outputs: &["view", "crossings"],
        entry: "flatten + composite",
        dispatch: Dispatch::Sequence(&[
            Dispatch::Linear {
                group: GroupSize::Entry("flatten_fill"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("composite"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("keep"),
            },
        ]),
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "bloom",
        kind: PassStage::Compute,
        inputs: &["view", "view_light"],
        outputs: &["bloom", "view"],
        entry: "bloom",
        dispatch: Dispatch::Sequence(&[
            Dispatch::Tiles {
                group: GroupSize::Entry("spread"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("down"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("up"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("show"),
            },
        ]),
        enabled_when: EnabledWhen(crate::render::SKIP_BLOOM),
    },
];

/// 光栅视图、**有链表或 slabs** 的 pass 序列（多几个中间 pass）。
pub const RASTER_LAYERED_PLAN: PassPlan = &[
    PassDecl {
        name: "shadow",
        kind: PassStage::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["shadow_layer"],
        entry: "shadow",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "occlusion depth",
        kind: PassStage::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["occlusion_depth"],
        entry: "occlusion depth",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "occlusion",
        kind: PassStage::Compute,
        inputs: &["occlusion_depth"],
        outputs: &["occlusion"],
        entry: "occlusion",
        dispatch: Dispatch::Tiles {
            group: GroupSize::Entry("sao"),
        },
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "opaque",
        kind: PassStage::Render,
        inputs: &["instances", "rows", "sprites"],
        outputs: &["view", "layers"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "append",
        kind: PassStage::Render,
        inputs: &["instances", "list_heads", "list_nodes", "list_appended", "layers"],
        outputs: &["list_heads", "list_nodes"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "slabs",
        kind: PassStage::Render,
        inputs: &["slab_bounds", "instances", "sprites"],
        outputs: &["layers"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "composite sprites",
        kind: PassStage::Render,
        inputs: &["instances", "rows", "sprites", "base"],
        outputs: &["view"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "base depth",
        kind: PassStage::Compute,
        inputs: &["base_samples", "base_sample_depths"],
        outputs: &["base_depth"],
        entry: "nearest_of_samples",
        dispatch: Dispatch::Tiles {
            group: GroupSize::Entry("nearest_of_samples"),
        },
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "vector composite",
        kind: PassStage::Compute,
        inputs: &["ctrl", "subs", "objects", "layers", "atlas", "base_depth", "crossings"],
        outputs: &["view", "crossings"],
        entry: "flatten + composite",
        dispatch: Dispatch::Sequence(&[
            Dispatch::Linear {
                group: GroupSize::Entry("flatten_fill"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("composite"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("keep"),
            },
        ]),
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "bloom",
        kind: PassStage::Compute,
        inputs: &["view", "view_light"],
        outputs: &["bloom", "view"],
        entry: "bloom",
        dispatch: Dispatch::Sequence(&[
            Dispatch::Tiles {
                group: GroupSize::Entry("spread"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("down"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("up"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("show"),
            },
        ]),
        enabled_when: EnabledWhen(crate::render::SKIP_BLOOM),
    },
];

/// 纯矢量视图的 pass 序列。
pub const VECTOR_PLAN: PassPlan = &[
    PassDecl {
        name: "atlas raster",
        kind: PassStage::Render,
        inputs: &["instances", "rows"],
        outputs: &["atlas"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "vector composite",
        kind: PassStage::Compute,
        inputs: &["ctrl", "subs", "objects", "atlas", "crossings"],
        outputs: &["view", "crossings"],
        entry: "flatten + composite",
        dispatch: Dispatch::Sequence(&[
            Dispatch::Linear {
                group: GroupSize::Entry("flatten_fill"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("composite"),
            },
            Dispatch::Tiles {
                group: GroupSize::Entry("keep"),
            },
        ]),
        enabled_when: EnabledWhen::ALWAYS,
    },
];

/// 环境预滤波（每帧最多一次，按需）。
pub const ENVIRONMENT_PLAN: PassPlan = &[PassDecl {
    name: "environment",
    kind: PassStage::Compute,
    inputs: &["environment"],
    outputs: &["environment"],
    entry: "environment",
    dispatch: Dispatch::Cube {
        group: GroupSize::Entry("to_cube"),
        faces: 6,
    },
    enabled_when: EnabledWhen(crate::render::SKIP_ENVIRONMENT),
}];

/// 着色 codegen 的两个 compute pass（按需，每帧可能多次）。
pub const CODEGEN_PLAN: PassPlan = &[
    PassDecl {
        name: "fill codegen",
        kind: PassStage::Compute,
        inputs: &["instances", "shader_programs"],
        outputs: &["instances"],
        entry: "fill codegen",
        dispatch: Dispatch::Fixed,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "frag codegen",
        kind: PassStage::Compute,
        inputs: &["instances", "shader_programs", "shader_colors"],
        outputs: &["shader_colors"],
        entry: "frag codegen",
        // 生成着色器：`@workgroup_size` 由 codegen 拼进源码，不在 WORKGROUP 表里 ⇒ 字面量。
        dispatch: Dispatch::Tiles {
            group: GroupSize::Literal(8),
        },
        enabled_when: EnabledWhen::ALWAYS,
    },
];

// ─── 校验 ────────────────────────────────────────────────────────────────

/// 声明的结构问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// 同一计划里出现重名 pass。
    Duplicate { name: &'static str, first: usize, again: usize },
    /// 某个 pass 读的资源没有任何前序 pass 生产（首帧即读不到）。
    Unproduced { pass: &'static str, input: &'static str },
    /// 资源依赖成环（附参与链，按声明下标）。
    Cycle { chain: Vec<usize> },
    /// 声明了 `Indirect`，但它的参数来源在前面没有任何生产者（会照着上一帧的垃圾值派发）。
    IndirectUnproduced {
        pass: &'static str,
        source: &'static str,
    },
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate { name, first, again } => write!(
                f,
                "pass 重名 {name:?}：声明 #{first} 与 #{again} 都叫这个名字"
            ),
            Self::Unproduced { pass, input } => write!(
                f,
                "pass {pass:?} 读的资源 {input:?} 在前面没有任何生产者（声明序即执行序）"
            ),
            Self::Cycle { chain } => write!(f, "pass 资源依赖成环（下标链）：{chain:?}"),
            Self::IndirectUnproduced { pass, source } => write!(
                f,
                "pass {pass:?} 用 indirect 参数 {source:?} 派发，但前面没有任何 pass 生产它\
                 （会照着上一帧留下的值派发）"
            ),
        }
    }
}

/// 校验一张计划：重名 → 输入必须有前序生产者 → 无环。
///
/// 注意：`inputs` 里出现在计划**外部**的名字（如 `instances` / `rows`，由帧前上传
/// 提供）不要求有生产者 —— 只对"同一计划内某个 pass 的 outputs"做前置性检查。
pub fn validate(plan: PassPlan) -> Result<(), PlanError> {
    // ① 重名。
    for (again, decl) in plan.iter().enumerate() {
        if let Some(first) = plan[..again].iter().position(|d| d.name == decl.name) {
            return Err(PlanError::Duplicate {
                name: decl.name,
                first,
                again,
            });
        }
    }

    // ② 环检测（资源级）：pass i --writes--> 资源 --read by--> pass j。
    // 先查环：成环时"前序生产者"必然也报错，环才是根因，先报根因。
    let mut adjacency: Adjacency<usize> = Adjacency::new();
    for (index, _) in plan.iter().enumerate() {
        adjacency.add_node(index);
    }
    for (producer, decl) in plan.iter().enumerate() {
        for (consumer, other) in plan.iter().enumerate() {
            if producer == consumer {
                continue;
            }
            if decl
                .outputs
                .iter()
                .any(|output| other.inputs.contains(output))
            {
                adjacency.add_edge(producer, consumer);
            }
        }
    }
    if let Some(chain) = find_cycle(&adjacency) {
        return Err(PlanError::Cycle { chain });
    }

    // ③ 前序生产者：只对"计划内某处会生产"的资源做前置性检查；
    // 同一 pass 读写同一资源（读-改-写）算自己是生产者。
    for (index, decl) in plan.iter().enumerate() {
        for input in decl.inputs {
            let produced_anywhere = plan.iter().any(|d| d.outputs.contains(input));
            let produced_before = plan
                .iter()
                .enumerate()
                .any(|(earlier, d)| earlier <= index && d.outputs.contains(input));
            if produced_anywhere && !produced_before {
                return Err(PlanError::Unproduced {
                    pass: decl.name,
                    input,
                });
            }
        }
    }
    // ④ Indirect：其参数来源必须由**更早**的 pass 生产。
    // 现状唯一一处 indirect（`settle_crossings`）由 `count_crossings` 无条件先写，所以安全；
    // 这条校验是为"铺开时"准备的：消费者跑在生产者之前，会照着上一帧留下的值派发。
    for (index, decl) in plan.iter().enumerate() {
        if let Dispatch::Indirect { source } = decl.dispatch {
            let produced_before = plan
                .iter()
                .enumerate()
                .any(|(earlier, d)| earlier < index && d.outputs.contains(&source));
            if !produced_before {
                return Err(PlanError::IndirectUnproduced {
                    pass: decl.name,
                    source,
                });
            }
        }
    }
    Ok(())
}

// ─── 运行时调度校验 ──────────────────────────────────────────────────────

/// 调度期错误：实际记录与声明不一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// 记录的名字与声明序不符（声明是顺序的唯一来源）。
    OutOfOrder {
        expected: &'static str,
        recorded: &'static str,
        index: usize,
    },
    /// 记录了一个被 `enabled_when` 关掉的 pass。
    Disabled { name: &'static str },
    /// 记录了一个不在本计划里的 pass。
    Undeclared { name: &'static str },
    /// 计划里该跑的 pass 没有被记录（静默跳过）。
    Missing { name: &'static str },
    /// **少绑**（BP-2）：pass 声明读某个资源，但本帧没有绑定它所属的组。
    Unbound {
        pass: &'static str,
        group: &'static str,
    },
    /// **多绑**（BP-2）：pass 绑了一个组，但它的声明里没有任何一个资源名来自这个组。
    Surplus {
        pass: &'static str,
        group: &'static str,
    },
    /// 记录了一个组名表里没有的组（名字对不上表）。
    UnknownGroup {
        pass: &'static str,
        group: &'static str,
    },
    /// 在任何一个 pass 之外记录了绑定（记账位置错了）。
    BindOutsidePass { group: &'static str },
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfOrder {
                expected,
                recorded,
                index,
            } => write!(
                f,
                "记录顺序与声明不符：第 {index} 个 pass 应是 {expected:?}，实际记录了 {recorded:?}"
            ),
            Self::Disabled { name } => {
                write!(f, "pass {name:?} 已被 enabled_when 关掉，却仍被记录（不静默执行）")
            }
            Self::Undeclared { name } => write!(f, "pass {name:?} 不在本帧的计划里"),
            Self::Missing { name } => {
                write!(f, "计划里的 pass {name:?} 没有被记录（静默跳过）")
            }
            Self::Unbound { pass, group } => write!(
                f,
                "pass {pass:?} 声明读的资源在组 {group:?} 里，但本帧没有绑定这个组（少绑）"
            ),
            Self::Surplus { pass, group } => write!(
                f,
                "pass {pass:?} 绑定了组 {group:?}，但它的声明里没有任何资源来自这个组（多绑）"
            ),
            Self::UnknownGroup { pass, group } => write!(
                f,
                "pass {pass:?} 绑定的组 {group:?} 不在组名表里（名字对不上，无法校验）"
            ),
            Self::BindOutsidePass { group } => write!(
                f,
                "组 {group:?} 的绑定记录在任何 pass 之外（记账位置错了）"
            ),
        }
    }
}

/// 一次帧内的调度游标：`begin(name)` 逐条核对，`finish()` 核对完整性。
///
/// 用法（`render.rs` 的帧循环）：
/// ```ignore
/// let mut schedule = PassSchedule::new(plan, skipping());
/// // ……每个 pass 记录前：
/// schedule.begin("shadow")?;
/// ```
/// 校验失败**返回错误**而不是 panic：调用方可以把它变成帧错误，且用例能直接断言。
#[derive(Debug)]
pub struct PassSchedule {
    plan: PassPlan,
    skip: u32,
    cursor: usize,
    recorded: Vec<&'static str>,
    skipped: Vec<&'static str>,
    raw_recorded: Vec<&'static str>,
    /// 名字表：声明名 → 组（空表 = 不做绑定校验，见 [`Self::new`]）。
    groups: BindSymbols,
    /// 实际绑定的记录：`(pass, 组名)`，由 [`Self::note_bind`] 在绑定处自动产生。
    bound: Vec<(&'static str, &'static str)>,
    /// 在任何 pass 之外记下的组名（`finish` 报错）。
    stray: Vec<&'static str>,
    /// 当前正在记录的 pass（`note_bind` 归到它名下）。
    current: Option<&'static str>,
}

impl PassSchedule {
    /// 不带组名表：只校验顺序与完整性（绑定校验留给 [`Self::with_groups`]）。
    pub fn new(plan: PassPlan, skip: u32) -> Self {
        Self::with_groups(plan, skip, BindSymbols::new(&[]))
    }

    /// 带组名表：`finish` 会比对「实际绑定」与「声明」。
    ///
    /// 只校验**参与记账**的 pass：一个 pass 只要本帧记过至少一次绑定，就算接入；
    /// 一次都没记的 pass 不参与比对（那是还没改造的调用点 —— 由 [`Self::unbound_passes`] 点名）。
    pub fn with_groups(plan: PassPlan, skip: u32, groups: BindSymbols) -> Self {
        Self {
            plan,
            skip,
            cursor: 0,
            recorded: Vec::new(),
            skipped: Vec::new(),
            raw_recorded: Vec::new(),
            groups,
            bound: Vec::new(),
            stray: Vec::new(),
            current: None,
        }
    }

    /// 已记录的数量（诊断用）。
    pub fn recorded(&self) -> &[&'static str] {
        &self.recorded
    }

    /// 通过逃生舱手写记录的 pass（诊断用；见 [`Self::raw`]）。
    pub fn raw_recorded(&self) -> &[&'static str] {
        &self.raw_recorded
    }

    /// 因**数据**缺席而显式跳过的 pass（诊断用）。
    pub fn skipped(&self) -> &[&'static str] {
        &self.skipped
    }

    /// 记录过的 pass 里**没有接入绑定记账**的那些（BP-2 / BP-4 的 escape 清单）。
    ///
    /// 它们照常通过顺序与完整性校验，但不参与"声明 vs 实际"比对 —— 名单本身就是账。
    pub fn unbound_passes(&self) -> Vec<&'static str> {
        self.recorded
            .iter()
            .copied()
            .filter(|pass| !self.bound.iter().any(|(p, _)| p == pass))
            .collect()
    }

    /// 本帧记过绑定账的 pass 数（诊断用）。
    pub fn bound_passes(&self) -> usize {
        self.recorded
            .iter()
            .filter(|pass| self.bound.iter().any(|(p, _)| p == *pass))
            .count()
    }

    /// **记一笔绑定**（BP-2）：在 `set_bind_group` 的同一处调用，归到当前 pass 名下。
    ///
    /// 它不执行绑定（渲染 pass 与计算 pass 的 `set_bind_group` 没有共同 trait），
    /// 但**紧接着**那次调用 —— 一致性数据由行为产生，调用方不申报期望值。
    pub fn note_bind(&mut self, group: &'static str) {
        match self.current {
            Some(pass) => self.bound.push((pass, group)),
            None => self.stray.push(group),
        }
    }

    /// 该 pass 声明的启用状态。
    pub fn enabled(&self, name: &str) -> bool {
        self.plan
            .iter()
            .find(|decl| decl.name == name)
            .is_some_and(|decl| decl.enabled_when.is_enabled(self.skip))
    }

    /// 推进到声明序里的**下一个启用项**，返回它。
    fn advance(&mut self) -> Result<&'static PassDecl, ScheduleError> {
        while self.cursor < self.plan.len()
            && !self.plan[self.cursor].enabled_when.is_enabled(self.skip)
        {
            self.cursor += 1;
        }
        self.plan
            .get(self.cursor)
            .ok_or(ScheduleError::Undeclared { name: "" })
    }

    /// 归因：名字在计划里、但被 `enabled_when` 关掉。
    fn disabled(&self, name: &str) -> bool {
        self.plan
            .iter()
            .find(|decl| decl.name == name)
            .is_some_and(|decl| !decl.enabled_when.is_enabled(self.skip))
    }

    /// 记录一个 pass：必须正好是声明序里下一个**启用**的 pass。
    pub fn begin(&mut self, name: &'static str) -> Result<(), ScheduleError> {
        let expected = match self.advance() {
            Ok(decl) => decl,
            Err(_) => return Err(ScheduleError::Undeclared { name }),
        };
        if expected.name != name {
            if self.disabled(name) {
                return Err(ScheduleError::Disabled { name });
            }
            return Err(ScheduleError::OutOfOrder {
                expected: expected.name,
                recorded: name,
                index: self.cursor,
            });
        }
        self.cursor += 1;
        self.recorded.push(name);
        self.current = Some(name);
        Ok(())
    }

    /// **数据驱动**地推进：`present` 为假表示这一步本帧没有内容（例如没有阴影投射者）。
    ///
    /// - 该 pass 被 `enabled_when` 关掉 ⇒ 不参与本帧，直接 `Ok(())`（不是错误）；
    /// - 否则必须正好是下一个启用项：`present` 决定"记录"还是"显式跳过"。
    ///
    /// 显式跳过与静默跳过的区别：跳过会被记进 `skipped`，`finish` 不会报 `Missing`；
    /// 而一个既没记录、也没显式跳过的启用项会被 `finish` 抓住。
    pub fn step(&mut self, name: &'static str, present: bool) -> Result<(), ScheduleError> {
        if self.disabled(name) {
            return Ok(());
        }
        let expected = match self.advance() {
            Ok(decl) => decl,
            Err(_) => return Err(ScheduleError::Undeclared { name }),
        };
        if expected.name != name {
            return Err(ScheduleError::OutOfOrder {
                expected: expected.name,
                recorded: name,
                index: self.cursor,
            });
        }
        self.cursor += 1;
        if present {
            self.recorded.push(name);
            self.current = Some(name);
        } else {
            self.skipped.push(name);
            self.current = None;
        }
        Ok(())
    }

    /// **逃生舱**：声明序照常推进，但这一步由手写 wgpu 记录。
    ///
    /// 用于声明式表达不了的东西（SDF 的 dispatch 计算、精细的 MRT 配置）。
    /// 与 `step(name, true)` 的区别只在记账：进 `raw_recorded`，与"声明式记录"
    /// 分开统计 —— 这样能看出有多少 pass 已经脱离声明，而不至于把逃生舱当成常态。
    pub fn raw(&mut self, name: &'static str) -> Result<(), ScheduleError> {
        if self.disabled(name) {
            return Err(ScheduleError::Disabled { name });
        }
        let expected = match self.advance() {
            Ok(decl) => decl,
            Err(_) => return Err(ScheduleError::Undeclared { name }),
        };
        if expected.name != name {
            return Err(ScheduleError::OutOfOrder {
                expected: expected.name,
                recorded: name,
                index: self.cursor,
            });
        }
        self.cursor += 1;
        self.raw_recorded.push(name);
        self.current = Some(name);
        Ok(())
    }

    /// 帧末核对：所有启用项都被记录或被显式跳过；接入记账的 pass 的绑定与声明一致。
    pub fn finish(mut self) -> Result<(), ScheduleError> {
        while self.cursor < self.plan.len() {
            let decl = &self.plan[self.cursor];
            if decl.enabled_when.is_enabled(self.skip) {
                return Err(ScheduleError::Missing { name: decl.name });
            }
            self.cursor += 1;
        }
        self.check_bindings()
    }

    /// **声明 vs 实际**（BP-2）。
    ///
    /// 只看本帧记过绑定的 pass（`bound` 里出现过的名字）。对每个这样的 pass：
    /// 1. 声明里的每个 **input**，若它属于表里某个组 ⇒ 该组必须绑过（少绑即红）；
    /// 2. 绑过的每个组，若它带可声明资源 ⇒ 声明里至少要有一个名字来自它（多绑即红）。
    ///
    /// `outputs` 是写入目标，不会被 `set_bind_group` 绑定，故不参与比对。
    fn check_bindings(&self) -> Result<(), ScheduleError> {
        if let Some(group) = self.stray.first() {
            return Err(ScheduleError::BindOutsidePass { group });
        }
        for decl in self.plan {
            let bound: Vec<&'static str> = self
                .bound
                .iter()
                .filter(|(pass, _)| *pass == decl.name)
                .map(|(_, group)| *group)
                .collect();
            if bound.is_empty() {
                continue;
            }
            for input in decl.inputs {
                if let Some(symbol) = self.groups.symbol_of_resource(input) {
                    if !bound.contains(&symbol.name) {
                        return Err(ScheduleError::Unbound {
                            pass: decl.name,
                            group: symbol.name,
                        });
                    }
                }
            }
            for group in &bound {
                let Some(symbol) = self.groups.symbol(group) else {
                    return Err(ScheduleError::UnknownGroup {
                        pass: decl.name,
                        group,
                    });
                };
                if symbol.resources.is_empty() {
                    continue;
                }
                if !symbol.resources.iter().any(|r| decl.inputs.contains(r)) {
                    return Err(ScheduleError::Surplus {
                        pass: decl.name,
                        group,
                    });
                }
            }
        }
        Ok(())
    }
}

// ─── Pod 侧：pass 提供者 ─────────────────────────────────────────────────

/// 一个 pass 提供者：声明"我提供哪些 pass 计划"。
///
/// # 为什么不是 `Pod::requires`
///
/// `ze_pod_core::ServiceRequirement` 装的是**服务**语义：一次注入、长期存在；
/// 而 pass 的输入资源是**一帧内生产-消费**、可别名、可被后续 pass 覆盖。
/// 两者生命周期不同，混进 `requires` 会让"资源缺了"与"服务缺了"无法区分。
/// 因此资源边单开一类（`inputs` / `outputs`），协商仍走"全有或全无"。
pub trait PassProvider {
    /// 稳定的提供者名（错误信息与去重用）。
    fn provider(&self) -> &'static str;

    /// 提供的计划。每个计划自带顺序（声明序即执行序）。
    fn plans(&self) -> Vec<(&'static str, PassPlan)>;
}

/// 内置提供者：本 crate 自己那几张计划。
pub struct BuiltinPasses;

impl PassProvider for BuiltinPasses {
    fn provider(&self) -> &'static str {
        "zela-render-vector"
    }

    fn plans(&self) -> Vec<(&'static str, PassPlan)> {
        vec![
            ("raster", RASTER_PLAN),
            ("raster.layered", RASTER_LAYERED_PLAN),
            ("vector", VECTOR_PLAN),
            ("environment", ENVIRONMENT_PLAN),
            ("codegen", CODEGEN_PLAN),
        ]
    }
}

/// 协商失败：**全有或全无**（缺一项就整批失败，不静默降级）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// 两个提供者声明了同名计划。
    DuplicatePlan { name: &'static str, first: &'static str, again: &'static str },
    /// 某张计划自身非法。
    Invalid { plan: &'static str, error: PlanError },
    /// 计划读的资源没有任何提供者生产。
    MissingResource { plan: &'static str, input: &'static str },
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicatePlan { name, first, again } => write!(
                f,
                "计划重名 {name:?}：{first} 与 {again} 都提供它（不允许同名覆盖）"
            ),
            Self::Invalid { plan, error } => write!(f, "计划 {plan:?} 非法：{error}"),
            Self::MissingResource { plan, input } => write!(
                f,
                "计划 {plan:?} 读的资源 {input:?} 没有任何提供者生产（全有或全无，不降级）"
            ),
        }
    }
}

/// 汇总多个提供者的计划并校验：重名 → 每张计划自身合法 → 资源要么由帧前提供、
/// 要么由同批某个 pass 生产。
///
/// `frame_resources` 是帧前就绪的资源名（实例缓冲、曲线控制点等）——它们不要求生产者。
pub fn negotiate(
    providers: &[&dyn PassProvider],
    frame_resources: &[&'static str],
) -> Result<Vec<(&'static str, PassPlan)>, ProviderError> {
    let mut plans: Vec<(&'static str, &'static str, PassPlan)> = Vec::new();
    for provider in providers {
        for (name, plan) in provider.plans() {
            if let Some((_, first, _)) = plans.iter().find(|(n, _, _)| *n == name) {
                return Err(ProviderError::DuplicatePlan {
                    name,
                    first,
                    again: provider.provider(),
                });
            }
            plans.push((name, provider.provider(), plan));
        }
    }

    let mut produced: Vec<&'static str> = frame_resources.to_vec();
    for (name, _, plan) in &plans {
        validate(plan).map_err(|error| ProviderError::Invalid { plan: name, error })?;
        for decl in *plan {
            produced.extend(decl.outputs.iter().copied());
        }
    }

    for (name, _, plan) in &plans {
        for decl in *plan {
            for input in decl.inputs {
                if !produced.contains(input) {
                    return Err(ProviderError::MissingResource {
                        plan: name,
                        input,
                    });
                }
            }
        }
    }

    Ok(plans
        .into_iter()
        .map(|(name, _, plan)| (name, plan))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 槽位快照（BN-0 第一项） ──────────────────────────────────────────

    /// 组 1 的编号逐项固定：值取自加本模块之前的 24 个手写 entry。
    ///
    /// 这条用例就是"重构前先固化现状"的那份快照 —— 槽位分配换成声明序之后，
    /// 任何一项编号变化都会在这里红。
    #[test]
    fn image_slots_are_pinned() {
        let expected: [(&str, u32); 24] = [
            ("atlas", 0),
            ("layers", 1),
            ("base", 2),
            ("view_out", 3),
            ("base_depth", 4),
            ("heads", 5),
            ("nodes", 6),
            ("base_samples", 7),
            ("base_sample_depths", 8),
            ("stroke_depths_in", 9),
            ("shadow_maps", 10),
            ("shadow_compare", 11),
            ("dfg_table", 12),
            ("dfg_linear", 13),
            ("crossings", 14),
            ("environment", 15),
            ("occlusion", 16),
            ("base_light", 17),
            ("base_light_samples", 18),
            ("light_out", 19),
            ("slab_bounds", 20),
            ("slabs", 21),
            ("shader_programs", 22),
            ("shader_colors", 23),
        ];
        for (name, slot) in expected {
            assert_eq!(image_slot(name), slot, "{name} 的槽位变了");
        }
    }

    /// 槽位稠密：编号 0..N-1 全覆盖，且名字不重复。
    #[test]
    fn image_slots_are_dense_and_unique() {
        assert_eq!(IMAGE_SLOTS.len(), IMAGE_SLOT_COUNT as usize);
        for (index, slot) in IMAGE_SLOTS.iter().enumerate() {
            assert_eq!(image_slot(slot.name), index as u32);
        }
        let mut names: Vec<&str> = IMAGE_SLOTS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), IMAGE_SLOTS.len(), "槽位名必须唯一");
    }

    /// 未登记的名字 ⇒ 显式 panic（不静默给 0 —— 那会静默画错纹理）。
    #[test]
    #[should_panic(expected = "没有登记资源")]
    fn unknown_resource_name_panics() {
        image_slot("no_such_texture");
    }

    // ── 顺序契约（BN-0 第二项） ──────────────────────────────────────────

    /// 无依赖边的 pass 保持**声明序**：调度按计划下标推进，不按图排序。
    #[test]
    fn undeclared_order_is_declaration_order() {
        let plan: PassPlan = &[
            PassDecl {
                name: "b",
                kind: PassStage::Compute,
                inputs: &[],
                outputs: &["x"],
                entry: "b",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "a",
                kind: PassStage::Compute,
                inputs: &[],
                outputs: &["y"],
                entry: "a",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
        ];
        let mut schedule = PassSchedule::new(plan, 0);
        schedule.begin("b").expect("声明在前");
        schedule.begin("a").expect("声明在后");
        assert_eq!(schedule.recorded(), &["b", "a"]);
        schedule.finish().expect("两条都记录了");
    }

    /// 反序记录 ⇒ `OutOfOrder`（并点名期望与实际的 pass）。
    #[test]
    fn wrong_order_is_reported() {
        let mut schedule = PassSchedule::new(VECTOR_PLAN, 0);
        let error = schedule.begin("composite").unwrap_err();
        match error {
            ScheduleError::OutOfOrder { expected, recorded, index } => {
                assert_eq!(expected, "atlas raster");
                assert_eq!(recorded, "composite");
                assert_eq!(index, 0);
            }
            other => panic!("期望 OutOfOrder，实际 {other:?}"),
        }
    }

    /// 计划里有、但没有记录 ⇒ `finish` 报 `Missing`（静默跳过会被抓住）。
    #[test]
    fn missing_pass_is_reported() {
        let mut schedule = PassSchedule::new(VECTOR_PLAN, 0);
        schedule.begin("atlas raster").unwrap();
        let error = schedule.finish().unwrap_err();
        assert_eq!(
            error,
            ScheduleError::Missing {
                name: "vector composite"
            }
        );
    }

    // ── enabled_when（BN-0 第四项） ──────────────────────────────────────

    /// 数据驱动的显式跳过：`step(name, false)` 计入 `skipped`，`finish` 不报 `Missing`。
    ///
    /// 这是"没有阴影投射者"这类情形的唯一合法路径 —— 既不静默，也不误报。
    #[test]
    fn step_without_content_is_explicit_skip() {
        let mut schedule = PassSchedule::new(VECTOR_PLAN, 0);
        schedule.step("atlas raster", false).unwrap();
        schedule.step("vector composite", true).unwrap();
        assert_eq!(schedule.recorded(), &["vector composite"]);
        assert_eq!(schedule.skipped(), &["atlas raster"]);
        schedule.finish().expect("显式跳过不算缺失");
    }

    /// 关闭的 pass 用 `step` 前进不算错误（数据驱动路径容忍 enabled_when）。
    #[test]
    fn step_tolerates_disabled_pass() {
        let mut schedule = PassSchedule::new(RASTER_PLAN, crate::render::SKIP_AO);
        schedule.step("shadow", true).unwrap();
        schedule.step("occlusion depth", false).unwrap();
        schedule.step("base", true).unwrap();
        assert_eq!(schedule.recorded(), &["shadow", "base"]);
        assert!(!schedule.skipped().contains(&"occlusion depth"));
    }

    /// 各 `SKIP_*` 位下被跳过的 pass 集合与声明一致（逐组合穷举）。
    #[test]
    fn enabled_sets_follow_skip_bits() {
        let skip_bits = [
            crate::render::SKIP_AO,
            crate::render::SKIP_SHADOW,
            crate::render::SKIP_BLOOM,
            crate::render::SKIP_ENVIRONMENT,
            crate::render::SKIP_LIGHT,
            crate::render::SKIP_DEPTH,
            crate::render::SKIP_STENCIL,
        ];
        // 所有 2^7 组合。
        for mask in 0u32..(1 << skip_bits.len()) {
            let skip = skip_bits
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .fold(0u32, |acc, (_, bit)| acc | bit);
            let enabled: Vec<&str> = RASTER_PLAN
                .iter()
                .filter(|decl| decl.enabled_when.is_enabled(skip))
                .map(|decl| decl.name)
                .collect();
            assert_eq!(
                enabled.contains(&"occlusion"),
                skip & crate::render::SKIP_AO == 0,
                "mask {mask:#b}：遮蔽应由 SKIP_AO 决定"
            );
            assert_eq!(
                enabled.contains(&"bloom"),
                skip & crate::render::SKIP_BLOOM == 0,
                "mask {mask:#b}：泛光应由 SKIP_BLOOM 决定"
            );
            // 其余 pass 无条件下执行。
            assert!(enabled.contains(&"shadow"));
            assert!(enabled.contains(&"base"));
        }
    }

    /// 关掉的 pass 若仍被记录 ⇒ `Disabled`（不静默执行）。
    #[test]
    fn recording_a_disabled_pass_is_reported() {
        let mut schedule =
            PassSchedule::new(RASTER_PLAN, crate::render::SKIP_AO | crate::render::SKIP_BLOOM);
        schedule.begin("shadow").unwrap();
        // occlusion depth / occlusion 被 SKIP_AO 关掉 ⇒ 下一步应是 base。
        let error = schedule.begin("occlusion depth").unwrap_err();
        assert_eq!(
            error,
            ScheduleError::Disabled {
                name: "occlusion depth"
            }
        );
    }

    // ── 计划校验（BN-3 纯数据） ──────────────────────────────────────────

    /// 内置计划全部合法：输入有前序生产者、无环、无重名。
    #[test]
    fn builtin_plans_are_valid() {
        for (name, plan) in [
            ("raster", RASTER_PLAN),
            ("raster.layered", RASTER_LAYERED_PLAN),
            ("vector", VECTOR_PLAN),
            ("environment", ENVIRONMENT_PLAN),
            ("codegen", CODEGEN_PLAN),
        ] {
            validate(plan).unwrap_or_else(|error| panic!("{name} 非法：{error}"));
        }
    }

    /// 读一个"计划内有生产者、但生产者排在后面"的资源 ⇒ `Unproduced`。
    #[test]
    fn read_before_production_is_reported() {
        let plan: PassPlan = &[
            PassDecl {
                name: "consumer",
                kind: PassStage::Compute,
                inputs: &["x"],
                outputs: &[],
                entry: "c",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "producer",
                kind: PassStage::Compute,
                inputs: &[],
                outputs: &["x"],
                entry: "p",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
        ];
        assert_eq!(
            validate(plan).unwrap_err(),
            PlanError::Unproduced {
                pass: "consumer",
                input: "x"
            }
        );
    }

    /// 资源依赖成环 ⇒ `Cycle`，且链上至少两个下标。
    #[test]
    fn resource_cycle_is_reported() {
        let plan: PassPlan = &[
            PassDecl {
                name: "a",
                kind: PassStage::Compute,
                inputs: &["y"],
                outputs: &["x"],
                entry: "a",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "b",
                kind: PassStage::Compute,
                inputs: &["x"],
                outputs: &["y"],
                entry: "b",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
        ];
        match validate(plan).unwrap_err() {
            PlanError::Cycle { chain } => assert!(chain.len() >= 2, "环链应含参与 pass：{chain:?}"),
            other => panic!("期望 Cycle，实际 {other:?}"),
        }
    }

    /// 重名 ⇒ `Duplicate`，并给出两处下标。
    #[test]
    fn duplicate_pass_is_reported() {
        let plan: PassPlan = &[
            PassDecl {
                name: "same",
                kind: PassStage::Compute,
                inputs: &[],
                outputs: &["x"],
                entry: "a",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "same",
                kind: PassStage::Compute,
                inputs: &[],
                outputs: &["y"],
                entry: "b",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
        ];
        assert_eq!(
            validate(plan).unwrap_err(),
            PlanError::Duplicate {
                name: "same",
                first: 0,
                again: 1
            }
        );
    }

    /// 逃生舱：手写记录的 pass 仍占声明序，但单独记账（`raw_recorded`）。
    #[test]
    fn raw_pass_keeps_declaration_order() {
        let mut schedule = PassSchedule::new(VECTOR_PLAN, 0);
        schedule.raw("atlas raster").expect("手写记录照常推进声明序");
        schedule.step("vector composite", true).unwrap();
        assert_eq!(schedule.recorded(), &["vector composite"]);
        assert_eq!(schedule.raw_recorded(), &["atlas raster"]);
        schedule.finish().expect("手写记录也算兑现了那一项");
    }

    /// 逃生舱不能跳过顺序：手写也必须按声明序推进。
    #[test]
    fn raw_pass_cannot_reorder() {
        let mut schedule = PassSchedule::new(VECTOR_PLAN, 0);
        let error = schedule.raw("vector composite").unwrap_err();
        assert!(matches!(error, ScheduleError::OutOfOrder { .. }));
    }

    // ── 可扩展性终检（BN-6 的机械版） ────────────────────────────────────

    /// **新增一个 pass 不动已有声明**：追加即可，前缀逐字段不变，且新计划仍可调度。
    ///
    /// 这是"加 pass 的 diff 里不出现已有 pass 的修改"的可判定形态：
    /// 顺序来自声明表，新项只追加在末尾，已有项一个字段都不用碰。
    #[test]
    fn adding_a_pass_does_not_touch_existing_ones() {
        let before: Vec<PassDecl> = VECTOR_PLAN.to_vec();
        let mut extended = before.clone();
        extended.push(PassDecl {
            name: "noop",
            kind: PassStage::Compute,
            inputs: &[],
            outputs: &[],
            entry: "noop",
            dispatch: Dispatch::Fixed,
            enabled_when: EnabledWhen::ALWAYS,
        });
        assert_eq!(
            &extended[..before.len()],
            &before[..],
            "已有 pass 的声明必须逐字段不变"
        );

        let plan: PassPlan = Box::leak(extended.into_boxed_slice());
        validate(plan).expect("追加后的计划仍合法");

        let mut schedule = PassSchedule::new(plan, 0);
        schedule.step("atlas raster", true).unwrap();
        schedule.step("vector composite", true).unwrap();
        schedule.step("noop", true).unwrap();
        schedule.finish().expect("新 pass 追加在末尾，已有顺序不变");
    }

    // ── Indirect（BO-2） ─────────────────────────────────────────────────

    const COUNT: PassDecl = PassDecl {
        name: "count",
        kind: PassStage::Compute,
        inputs: &[],
        outputs: &["crossing groups"],
        entry: "count_crossings",
        dispatch: Dispatch::Fixed,
        enabled_when: EnabledWhen::ALWAYS,
    };
    const SETTLE: PassDecl = PassDecl {
        name: "settle",
        kind: PassStage::Compute,
        inputs: &[],
        outputs: &[],
        entry: "settle_crossings",
        dispatch: Dispatch::Indirect {
            source: "crossing groups",
        },
        enabled_when: EnabledWhen::ALWAYS,
    };

    /// 消费者跑在生产者之前 ⇒ `IndirectUnproduced`（会照着上一帧的值派发）。
    #[test]
    fn indirect_before_its_producer_is_reported() {
        let plan: PassPlan = &[SETTLE, COUNT];
        assert_eq!(
            validate(plan).unwrap_err(),
            PlanError::IndirectUnproduced {
                pass: "settle",
                source: "crossing groups"
            }
        );
    }

    /// 生产者在前 ⇒ 合法（这正是现状：`count_crossings` 无条件先写）。
    #[test]
    fn indirect_after_its_producer_is_valid() {
        let plan: PassPlan = &[COUNT, SETTLE];
        validate(plan).expect("生产者在前，indirect 的参数有来源");
    }

    // ── Pod 协商（BN-4 纯数据） ──────────────────────────────────────────

    struct TestProvider(&'static str, &'static str, PassPlan);

    impl PassProvider for TestProvider {
        fn provider(&self) -> &'static str {
            self.0
        }
        fn plans(&self) -> Vec<(&'static str, PassPlan)> {
            vec![(self.1, self.2)]
        }
    }

    /// 帧前就绪的资源（实例缓冲、曲线控制点、画布自带的采样纹理…）。
    ///
    /// 这些名不要求任何 pass 生产：它们由帧前的上传 / canvas 附件提供。
    const FRAME: &[&'static str] = &[
        "instances", "rows", "sprites", "ctrl", "subs", "objects", "tiles", "view_light",
        "environment", "shader_programs", "shader_colors", "slab_bounds", "base_samples",
        "base_sample_depths", "stroke_depths_in", "base", "base_light", "list_heads",
        "list_nodes", "list_appended",
    ];

    /// 内置提供者 + 帧前资源 ⇒ 协商成功。
    #[test]
    fn builtin_provider_negotiates() {
        let plans = negotiate(&[&BuiltinPasses], FRAME)
            .unwrap_or_else(|error| panic!("内置计划应协商成功：{error}"));
        assert_eq!(plans.len(), 5);
    }

    /// 两个提供者给同名计划 ⇒ 整批失败（不允许同名覆盖）。
    #[test]
    fn duplicate_plan_fails_whole_batch() {
        let other = TestProvider("other", "vector", VECTOR_PLAN);
        let error = negotiate(&[&BuiltinPasses, &other], FRAME).unwrap_err();
        assert_eq!(
            error,
            ProviderError::DuplicatePlan {
                name: "vector",
                first: "zela-render-vector",
                again: "other",
            }
        );
    }

    /// 缺一个输入资源 ⇒ 整批失败（全有或全无，不静默降级）。
    ///
    /// 用 `shader_programs`：它只被读、不被任何 pass 写出，所以缺了就真的没来源。
    #[test]
    fn missing_resource_fails_whole_batch() {
        let without = FRAME
            .iter()
            .copied()
            .filter(|name| *name != "shader_programs")
            .collect::<Vec<_>>();
        let error = negotiate(&[&BuiltinPasses], &without).unwrap_err();
        assert_eq!(
            error,
            ProviderError::MissingResource {
                plan: "codegen",
                input: "shader_programs"
            }
        );
    }

    // ── 声明 vs 实际绑定（BP-2） ─────────────────────────────────────────

    /// 一条两 pass 的玩具计划 + 它用的组名表（`scene` / `images` 各管一边，`lighting` 空清单）。
    const BOUND_GROUPS: BindSymbols = BindSymbols::new(&[
        GroupSymbol {
            group: 0,
            name: "scene",
            resources: &["instances", "rows", "sprites"],
        },
        GroupSymbol {
            group: 1,
            name: "images",
            resources: &["atlas", "base"],
        },
        GroupSymbol {
            group: 3,
            name: "lighting",
            resources: &[],
        },
    ]);

    const BOUND_PLAN: PassPlan = &[
        PassDecl {
            name: "draw",
            kind: PassStage::Render,
            inputs: &["instances", "rows", "sprites"],
            outputs: &["view"],
            entry: "draw",
            dispatch: Dispatch::Draw,
            enabled_when: EnabledWhen::ALWAYS,
        },
        PassDecl {
            name: "composite",
            kind: PassStage::Compute,
            inputs: &["instances", "atlas", "base"],
            outputs: &["view"],
            entry: "composite",
            dispatch: Dispatch::Fixed,
            enabled_when: EnabledWhen::ALWAYS,
        },
    ];

    /// 声明与绑定一致 ⇒ 通过（`lighting` 空清单不参与"多绑"判定）。
    #[test]
    fn bindings_match_the_declaration() {
        let mut schedule = PassSchedule::with_groups(BOUND_PLAN, 0, BOUND_GROUPS);
        schedule.step("draw", true).unwrap();
        schedule.note_bind("scene");
        schedule.note_bind("lighting");
        schedule.step("composite", true).unwrap();
        schedule.note_bind("scene");
        schedule.note_bind("images");
        assert_eq!(schedule.bound_passes(), 2);
        assert!(schedule.unbound_passes().is_empty());
        schedule.finish().expect("声明与绑定一致");
    }

    /// 证伪 1（少绑）：声明读 `atlas`，却只绑了 `scene` ⇒ `Unbound { group: "images" }`。
    #[test]
    fn a_declared_group_that_is_not_bound_is_red() {
        let mut schedule = PassSchedule::with_groups(BOUND_PLAN, 0, BOUND_GROUPS);
        schedule.step("draw", true).unwrap();
        schedule.note_bind("scene");
        schedule.step("composite", true).unwrap();
        schedule.note_bind("scene");
        assert_eq!(
            schedule.finish().unwrap_err(),
            ScheduleError::Unbound {
                pass: "composite",
                group: "images"
            }
        );
    }

    /// 证伪 2（多绑）：绑了 `images`，但声明里没有一个来自它的名字 ⇒ `Surplus`。
    #[test]
    fn a_bound_group_that_is_not_declared_is_red() {
        let mut schedule = PassSchedule::with_groups(BOUND_PLAN, 0, BOUND_GROUPS);
        schedule.step("draw", true).unwrap();
        schedule.note_bind("scene");
        schedule.note_bind("images");
        schedule.step("composite", true).unwrap();
        schedule.note_bind("scene");
        schedule.note_bind("images");
        assert_eq!(
            schedule.finish().unwrap_err(),
            ScheduleError::Surplus {
                pass: "draw",
                group: "images"
            }
        );
    }

    /// 证伪 3（改坏声明）：把 `draw` 的 `rows` 改成 `atlas` ⇒ 声明的 `atlas` 在 `images` 里，
    /// 而 `draw` 只绑了 `scene` ⇒ 红。
    #[test]
    fn breaking_a_declaration_is_red() {
        const BROKEN: PassPlan = &[
            PassDecl {
                name: "draw",
                kind: PassStage::Render,
                inputs: &["instances", "atlas", "sprites"],
                outputs: &["view"],
                entry: "draw",
                dispatch: Dispatch::Draw,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "composite",
                kind: PassStage::Compute,
                inputs: &["instances", "atlas", "base"],
                outputs: &["view"],
                entry: "composite",
                dispatch: Dispatch::Fixed,
                enabled_when: EnabledWhen::ALWAYS,
            },
        ];
        let mut schedule = PassSchedule::with_groups(BROKEN, 0, BOUND_GROUPS);
        schedule.step("draw", true).unwrap();
        schedule.note_bind("scene");
        schedule.step("composite", true).unwrap();
        schedule.note_bind("scene");
        schedule.note_bind("images");
        assert_eq!(
            schedule.finish().unwrap_err(),
            ScheduleError::Unbound {
                pass: "draw",
                group: "images"
            }
        );
    }

    /// 未接入记账的 pass 不参与比对，但会被 [`PassSchedule::unbound_passes`] 点名（escape 账）。
    #[test]
    fn unbound_passes_are_listed_not_checked() {
        let mut schedule = PassSchedule::with_groups(BOUND_PLAN, 0, BOUND_GROUPS);
        schedule.step("draw", true).unwrap();
        schedule.note_bind("scene");
        schedule.step("composite", true).unwrap();
        assert_eq!(schedule.unbound_passes(), vec!["composite"]);
        schedule.finish().expect("未接入的 pass 不产生绑定错误");
    }

    /// 绑定记录写在任何 pass 之外 ⇒ `BindOutsidePass`（记账位置错了）。
    #[test]
    fn binds_outside_a_pass_are_red() {
        let mut schedule = PassSchedule::with_groups(BOUND_PLAN, 0, BOUND_GROUPS);
        // 还没记录任何 pass 就先记账。
        schedule.note_bind("scene");
        schedule.step("draw", true).unwrap();
        schedule.note_bind("scene");
        schedule.step("composite", true).unwrap();
        schedule.note_bind("scene");
        schedule.note_bind("images");
        assert_eq!(
            schedule.finish().unwrap_err(),
            ScheduleError::BindOutsidePass { group: "scene" }
        );
    }

    /// 真实组名表自身合法，且 `images` 里的每个名字都是 `IMAGE_SLOTS` 的槽位。
    #[test]
    fn real_group_tables_are_valid() {
        RASTER_GROUPS.validate().expect("raster 组表非法");
        VECTOR_GROUPS.validate().expect("vector 组表非法");
        let images = VECTOR_GROUPS.symbol("images").expect("images 组");
        for name in images.resources {
            image_slot(name); // 未登记即 panic 并点名
        }
        assert!(
            !images.resources.is_empty(),
            "images 组必须带可声明资源，否则多绑判定形同虚设"
        );
    }

    /// 真实计划里出现在声明中的组 1 名字都能在 `VECTOR_GROUPS` 里查到组
    /// （否则"声明了却绑不上"会悄悄逃过校验）。
    #[test]
    fn declared_image_names_are_covered_by_the_vector_table() {
        for decl in VECTOR_PLAN {
            for input in decl.inputs {
                let known = IMAGE_SLOTS.iter().any(|slot| slot.name == *input);
                if known {
                    assert!(
                        VECTOR_GROUPS.symbol_of_resource(input).is_some(),
                        "pass {:?} 的输入 {input:?} 是组 1 槽位，但 vector 组表未覆盖",
                        decl.name
                    );
                }
            }
        }
    }
}
