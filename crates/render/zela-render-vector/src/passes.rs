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

// ─── 目标纹理身份 ────────────────────────────────────────────────────────

/// 一帧内被写入的纹理目标。名字用于声明与校验，**不承担分配职责**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
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

impl Target {
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
pub enum PassKind {
    /// 渲染 pass（有颜色 / 深度附件）。
    Render,
    /// 计算 pass。
    Compute,
}

impl PassKind {
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
#[derive(Debug, Clone, Copy)]
pub struct PassDecl {
    /// 唯一名字（运行时 recording 必须按这个名字报数）。
    pub name: &'static str,
    pub kind: PassKind,
    /// 读哪些逻辑资源（组 1 的槽位名，或上一 pass 的输出）。
    pub inputs: &'static [&'static str],
    /// 写哪些目标（`Target::name()`）。
    pub outputs: &'static [&'static str],
    /// 顶点 / 片段 / 计算入口名（纯记录：入口是 shader 的事实，不在这里推导）。
    pub entry: &'static str,
    /// dispatch 形状（纯记录，供文档与后续 codegen 使用）。
    pub dispatch: Dispatch,
    pub enabled_when: EnabledWhen,
}

/// dispatch 形状（声明用；本批不据此推导任何东西）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// 由顶点数 / 索引决定。
    Draw,
    /// 按像素铺满（`ceil(size / 64)` 类）。
    Pixels,
    /// 按视图像素一半（泛光的粗级）。
    HalfPixels,
    /// 按固定尺寸（全屏一次性）。
    Full,
}

/// 一帧的 pass 序列声明（顺序 = 执行顺序）。
pub type PassPlan = &'static [PassDecl];

/// 光栅视图、**无链表 / 无 slabs** 的 pass 序列。
pub const RASTER_PLAN: PassPlan = &[
    PassDecl {
        name: "shadow",
        kind: PassKind::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["shadow_layer"],
        entry: "shadow",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "occlusion depth",
        kind: PassKind::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["occlusion_depth"],
        entry: "occlusion depth",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "occlusion",
        kind: PassKind::Compute,
        inputs: &["occlusion_depth"],
        outputs: &["occlusion"],
        entry: "occlusion",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "base",
        kind: PassKind::Render,
        inputs: &["instances", "rows", "sprites"],
        outputs: &["view", "atlas_depth"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "base depth",
        kind: PassKind::Compute,
        inputs: &["base_samples", "base_sample_depths"],
        outputs: &["base_depth"],
        entry: "nearest_of_samples",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "flatten",
        kind: PassKind::Compute,
        inputs: &["ctrl", "subs", "objects", "base", "atlas", "base_depth"],
        outputs: &["crossings"],
        entry: "flatten",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "composite",
        kind: PassKind::Compute,
        inputs: &["crossings", "atlas", "layers", "base"],
        outputs: &["view"],
        entry: "composite",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "bloom",
        kind: PassKind::Compute,
        inputs: &["view", "view_light"],
        outputs: &["bloom", "view"],
        entry: "bloom",
        dispatch: Dispatch::HalfPixels,
        enabled_when: EnabledWhen(crate::render::SKIP_BLOOM),
    },
];

/// 光栅视图、**有链表或 slabs** 的 pass 序列（多几个中间 pass）。
pub const RASTER_LAYERED_PLAN: PassPlan = &[
    PassDecl {
        name: "shadow",
        kind: PassKind::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["shadow_layer"],
        entry: "shadow",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "occlusion depth",
        kind: PassKind::Render,
        inputs: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        outputs: &["occlusion_depth"],
        entry: "occlusion depth",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "occlusion",
        kind: PassKind::Compute,
        inputs: &["occlusion_depth"],
        outputs: &["occlusion"],
        entry: "occlusion",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen(crate::render::SKIP_AO),
    },
    PassDecl {
        name: "opaque",
        kind: PassKind::Render,
        inputs: &["instances", "rows", "sprites"],
        outputs: &["view", "layers"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "append",
        kind: PassKind::Render,
        inputs: &["instances", "list_heads", "list_nodes", "list_appended", "layers"],
        outputs: &["list_heads", "list_nodes"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "slabs",
        kind: PassKind::Render,
        inputs: &["slab_bounds", "instances", "sprites"],
        outputs: &["layers"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "composite sprites",
        kind: PassKind::Render,
        inputs: &["instances", "rows", "sprites", "base"],
        outputs: &["view"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "base depth",
        kind: PassKind::Compute,
        inputs: &["base_samples", "base_sample_depths"],
        outputs: &["base_depth"],
        entry: "nearest_of_samples",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "flatten",
        kind: PassKind::Compute,
        inputs: &["ctrl", "subs", "objects", "base", "atlas", "base_depth"],
        outputs: &["crossings"],
        entry: "flatten",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "composite",
        kind: PassKind::Compute,
        inputs: &["crossings", "atlas", "layers", "base"],
        outputs: &["view"],
        entry: "composite",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "bloom",
        kind: PassKind::Compute,
        inputs: &["view", "view_light"],
        outputs: &["bloom", "view"],
        entry: "bloom",
        dispatch: Dispatch::HalfPixels,
        enabled_when: EnabledWhen(crate::render::SKIP_BLOOM),
    },
];

/// 纯矢量视图的 pass 序列。
pub const VECTOR_PLAN: PassPlan = &[
    PassDecl {
        name: "atlas raster",
        kind: PassKind::Render,
        inputs: &["instances", "rows"],
        outputs: &["atlas"],
        entry: "vs_stroke",
        dispatch: Dispatch::Draw,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "flatten",
        kind: PassKind::Compute,
        inputs: &["ctrl", "subs", "objects", "atlas"],
        outputs: &["crossings"],
        entry: "flatten",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "composite",
        kind: PassKind::Compute,
        inputs: &["crossings", "atlas"],
        outputs: &["view"],
        entry: "composite",
        dispatch: Dispatch::Pixels,
        enabled_when: EnabledWhen::ALWAYS,
    },
];

/// 环境预滤波（每帧最多一次，按需）。
pub const ENVIRONMENT_PLAN: PassPlan = &[PassDecl {
    name: "environment",
    kind: PassKind::Compute,
    inputs: &["environment"],
    outputs: &["environment"],
    entry: "environment",
    dispatch: Dispatch::Full,
    enabled_when: EnabledWhen(crate::render::SKIP_ENVIRONMENT),
}];

/// 着色 codegen 的两个 compute pass（按需，每帧可能多次）。
pub const CODEGEN_PLAN: PassPlan = &[
    PassDecl {
        name: "fill codegen",
        kind: PassKind::Compute,
        inputs: &["instances", "shader_programs"],
        outputs: &["instances"],
        entry: "fill codegen",
        dispatch: Dispatch::Full,
        enabled_when: EnabledWhen::ALWAYS,
    },
    PassDecl {
        name: "frag codegen",
        kind: PassKind::Compute,
        inputs: &["instances", "shader_programs", "shader_colors"],
        outputs: &["shader_colors"],
        entry: "frag codegen",
        dispatch: Dispatch::Pixels,
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
}

impl PassSchedule {
    pub fn new(plan: PassPlan, skip: u32) -> Self {
        Self {
            plan,
            skip,
            cursor: 0,
            recorded: Vec::new(),
        }
    }

    /// 已记录的数量（诊断用）。
    pub fn recorded(&self) -> &[&'static str] {
        &self.recorded
    }

    /// 该 pass 声明的启用状态。
    pub fn enabled(&self, name: &str) -> bool {
        self.plan
            .iter()
            .find(|decl| decl.name == name)
            .is_some_and(|decl| decl.enabled_when.is_enabled(self.skip))
    }

    /// 记录一个 pass：必须正好是声明序里下一个**启用**的 pass。
    pub fn begin(&mut self, name: &'static str) -> Result<(), ScheduleError> {
        // 跳过被 enabled_when 关掉的声明（它们不会被记录）。
        while self.cursor < self.plan.len()
            && !self.plan[self.cursor].enabled_when.is_enabled(self.skip)
        {
            self.cursor += 1;
        }
        let Some(expected) = self.plan.get(self.cursor) else {
            return Err(ScheduleError::Undeclared { name });
        };
        if expected.name != name {
            // 更精确的归因：名字在计划里、但本轮已因 enabled_when 关掉。
            if let Some(decl) = self.plan.iter().find(|decl| decl.name == name) {
                if !decl.enabled_when.is_enabled(self.skip) {
                    return Err(ScheduleError::Disabled { name });
                }
            }
            return Err(ScheduleError::OutOfOrder {
                expected: expected.name,
                recorded: name,
                index: self.cursor,
            });
        }
        self.cursor += 1;
        self.recorded.push(name);
        Ok(())
    }

    /// 帧末核对：所有启用项都被记录过。
    pub fn finish(mut self) -> Result<(), ScheduleError> {
        while self.cursor < self.plan.len() {
            let decl = &self.plan[self.cursor];
            if decl.enabled_when.is_enabled(self.skip) {
                return Err(ScheduleError::Missing { name: decl.name });
            }
            self.cursor += 1;
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
                kind: PassKind::Compute,
                inputs: &[],
                outputs: &["x"],
                entry: "b",
                dispatch: Dispatch::Full,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "a",
                kind: PassKind::Compute,
                inputs: &[],
                outputs: &["y"],
                entry: "a",
                dispatch: Dispatch::Full,
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
                name: "flatten"
            }
        );
    }

    // ── enabled_when（BN-0 第四项） ──────────────────────────────────────

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
                kind: PassKind::Compute,
                inputs: &["x"],
                outputs: &[],
                entry: "c",
                dispatch: Dispatch::Full,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "producer",
                kind: PassKind::Compute,
                inputs: &[],
                outputs: &["x"],
                entry: "p",
                dispatch: Dispatch::Full,
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
                kind: PassKind::Compute,
                inputs: &["y"],
                outputs: &["x"],
                entry: "a",
                dispatch: Dispatch::Full,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "b",
                kind: PassKind::Compute,
                inputs: &["x"],
                outputs: &["y"],
                entry: "b",
                dispatch: Dispatch::Full,
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
                kind: PassKind::Compute,
                inputs: &[],
                outputs: &["x"],
                entry: "a",
                dispatch: Dispatch::Full,
                enabled_when: EnabledWhen::ALWAYS,
            },
            PassDecl {
                name: "same",
                kind: PassKind::Compute,
                inputs: &[],
                outputs: &["y"],
                entry: "b",
                dispatch: Dispatch::Full,
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
}
