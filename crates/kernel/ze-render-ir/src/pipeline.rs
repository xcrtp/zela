// pipeline.rs — 管线状态声明（B2-0b）+ 能力查询（B2-0c）。
//
// 为什么在 IR：管线状态是**意图层声明**（拓扑 / 着色变体 / 是否双面 / 混合语义 /
// 顶点布局），不是 GPU 命令。后端负责把它翻译成 `wgpu::RenderPipelineDescriptor`
// （作者与 IR 都不暴露 `cull_mode` / `blend` / `sample_count` / `format`）。
//
// 🔴 本文件**不得出现任何 wgpu 类型**（计划 #1 的直接修正、D19 的核心）。

use crate::layer::{LayerBlend, LayerId};
use crate::mesh::{PrimitiveTopology, ShaderVariant};

/// 背面剔除模式（后端中立的 `cull_mode` 表示）。
///
/// IR 侧通常只声明意图（[`PipelineDesc::double_sided`]），本枚举供后端翻译与
/// 显式状态查询使用；`Front` 保留给镜像/负缩放场景的显式需求。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CullMode {
    /// 不剔除（双面可见）。
    None,
    /// 剔除正面。
    Front,
    /// 剔除背面（默认的封闭实体语义）。
    Back,
}

/// 一条管线的状态声明（**纯数据**）。
///
/// - `double_sided`：意图层声明（D19）。`true` ⇒ 后端 `cull_mode = None`；
///   `false` ⇒ 后端 `cull_mode = Back`（三角形）/ `None`（线段、点）。
/// - `blend`：混合语义沿用 IR 既有 [`LayerBlend`]（#14 先例），后端翻译成
///   `BlendState`（`Opaque → REPLACE` / `Alpha → 预乘 over` / `Additive → One,One`）。
/// - `vertex_layout`：顶点布局契约（D23，ze-vertex）。同布局 + 同其余维度才共享
///   pipeline；队列合并后两套 Layout 共存，故布局是 key 的必要维度（D15）。
/// - `requirements`：该管线的顶点入口要求哪些语义（见 [`ShaderRequirements`]）。
///   与 `vertex_layout` 一起构成"shader 要什么 × mesh 给什么"的两侧输入。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PipelineDesc {
    pub topology: PrimitiveTopology,
    pub variant: ShaderVariant,
    /// 意图层：是否双面可见（`true` ⇒ 不剔除背面）。
    pub double_sided: bool,
    pub blend: LayerBlend,
    pub vertex_layout: ze_vertex::VertexLayout,
    /// 顶点入口的语义要求（顺序 = shader location 顺序）。
    pub requirements: ShaderRequirements,
}

// ─── 顶点语义需求（顶点入口 × 语义） ───────────────────────────────────────

/// 一个顶点入口对顶点语义的要求。
///
/// `semantics` 的**顺序即 shader `@location` 的顺序**（下标 0,1,2…）。
///
/// # 为什么需求放在 IR
///
/// 需求是"管线意图"的一部分，与 [`PipelineDesc`] 同层；而它的**事实来源**是 shader
/// 源码（`cube.wgsl` 的 `fn vs_*` 参数表）。两份表达必须由门禁对齐：
/// `zela-render-backends/tests/wgsl_requirements.rs` 解析源码里的 `@location(n)`，
/// 与本表的条目逐项比对，不一致即红。
///
/// 需求表不能放后端 crate：生产端（`zela-render`）需要它来补齐顶点，
/// 而依赖方向是 `zela-render-backends → zela-render`，反向拿不到。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShaderRequirements {
    pub semantics: Vec<ze_vertex::Semantic>,
}

/// 顶点入口：三角形（读位置 / UV / 着色法线）。
pub const VS_MAIN: &str = "vs_main";

/// 顶点入口：线段（只读位置；UV 与着色法线由入口内给常量）。
pub const VS_LINE: &str = "vs_line";

/// 顶点入口名 → 语义需求。未知入口返回 `None`（调用方显式报错，**不猜**）。
pub fn vs_requirements(entry: &str) -> Option<ShaderRequirements> {
    use ze_vertex::Semantic;
    let semantics = match entry {
        VS_MAIN => vec![Semantic::Position, Semantic::Uv, Semantic::ShadingNormal],
        VS_LINE => vec![Semantic::Position],
        _ => return None,
    };
    Some(ShaderRequirements { semantics })
}

/// 图元拓扑 → 顶点入口名。
///
/// - `Triangles` → [`VS_MAIN`]（位置 / UV / 法线）；
/// - `Lines` → [`VS_LINE`]（只有位置，stride 12）；
/// - `Points` → [`VS_MAIN`]：3D 后端显式拒绝点列表（无生产者），映射只为穷尽。
///
/// 入口名是 shader 的事实，集中在这里才有一处可查；`tests/wgsl_requirements.rs`
/// 核对"这张表里的入口在 `cube.wgsl` 里真实存在且 location 数量一致"。
pub fn vs_entry_for(topology: PrimitiveTopology) -> &'static str {
    match topology {
        PrimitiveTopology::Triangles | PrimitiveTopology::Points => VS_MAIN,
        PrimitiveTopology::Lines => VS_LINE,
    }
}

/// 拓扑 → 语义需求（[`vs_entry_for`] + [`vs_requirements`] 的组合）。
///
/// 入口未登记时 panic 并命名（不静默返回空需求）。
pub fn vs_requirements_for(topology: PrimitiveTopology) -> ShaderRequirements {
    let entry = vs_entry_for(topology);
    vs_requirements(entry).unwrap_or_else(|| {
        panic!("顶点入口 {entry:?} 没有登记语义需求（ShaderRequirements 表缺项）")
    })
}

/// 渲染通道的深度处理意图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassDepth {
    /// 不挂深度附件（2D 离屏 / 合成 pass）。
    None,
    /// 深度测试（Less）+ 写入（3D 两个 pass）。
    TestAndWrite,
}

/// 一个渲染通道的声明（**纯数据**）。
///
/// `sample_count` / `color_format` / `depth_format` **不在本结构里** —— 它们由
/// target 决定，属[能力](Capabilities)而非命令（D22）。
///
/// 注：`Color32` 未实现 `Hash`，故 `PassDesc` 不派发 `Hash`（通道声明不进
/// pipeline 缓存 key，无需可哈希）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassDesc {
    /// 目标图层（离屏目标按此 id 持有）。
    pub layer: LayerId,
    /// 清屏颜色。
    pub clear_color: ze_notion::Color32,
    pub depth: PassDepth,
}

// ─── B2-0c：能力查询（D22） ────────────────────────────────────────────────

/// 颜色目标格式（后端中立的有限集合；由 target 决定）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ColorFormat {
    /// sRGB 8bit（默认窗口/离屏目标）。
    Rgba8UnormSrgb,
    /// 线性 8bit（回读/测试目标）。
    Rgba8Unorm,
    /// sRGB 8bit（原生窗口常见目标格式，B3-0.5b 补齐）。
    Bgra8UnormSrgb,
    /// 线性 8bit（部分平台窗口目标）。
    Bgra8Unorm,
}

/// 深度格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepthFormat {
    Depth32Float,
}

/// 后处理 / IBL 的**后端自述**能力（B4 Step 1）。
///
/// 未实现的后端一律 `none()`；`RenderParams::is_supported_by` 用它拦下不支持的参数，
/// **不静默降级**（§0.3 ②）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PostCaps {
    pub tone: bool,
    pub bloom: bool,
    pub ao: bool,
    pub env: bool,
}

impl PostCaps {
    /// 全部不支持（wgpu 三角化后端当前形态）。
    pub const fn none() -> Self {
        Self {
            tone: false,
            bloom: false,
            ao: false,
            env: false,
        }
    }

    /// 全部支持（矢量后端移植后的形态）。
    pub const fn all() -> Self {
        Self {
            tone: true,
            bloom: true,
            ao: true,
            env: true,
        }
    }
}

/// 后端声明的**能力**（不是命令）：target 相关的采样数 / 格式。
///
/// 消费者（如 B2-5 的 AA 统一）先查询，再决定请求值；**不支持 ⇒ 显式报错**
/// （不支持就显式报错），不得静默降级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Capabilities {
    /// 目标颜色格式支持的最大 MSAA 采样数（1 = 无 MSAA）。
    pub max_sample_count: u32,
    pub color_format: ColorFormat,
    pub depth_format: DepthFormat,
    /// 后处理 / IBL 能力（B4 Step 1）。
    pub post: PostCaps,
}

impl Capabilities {
    /// 请求 `requested` 采样数：≤ 最大支持 ⇒ 原样返回；否则**显式报错**并命名。
    pub fn require_sample_count(&self, requested: u32) -> Result<u32, CapabilityError> {
        if requested == 0 || !requested.is_power_of_two() {
            return Err(CapabilityError::InvalidSampleCount { requested });
        }
        if requested > self.max_sample_count {
            return Err(CapabilityError::UnsupportedSampleCount {
                requested,
                max_supported: self.max_sample_count,
            });
        }
        Ok(requested)
    }
}

/// 能力不满足时的显式错误（不支持就命名报错，不静默降级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityError {
    #[error("采样数 {requested} 非法（必须为 2 的幂且 ≥ 1）")]
    InvalidSampleCount { requested: u32 },
    #[error("后端目标最多支持 MSAA {max_supported}x，请求了 {requested}x（不静默降级）")]
    UnsupportedSampleCount {
        requested: u32,
        max_supported: u32,
    },
    #[error("目标颜色格式不受 IR 能力声明支持（仅 Rgba8UnormSrgb / Rgba8Unorm；不静默）")]
    UnsupportedColorFormat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_vertex::{AaModel, CacheStrategy, VertexLayout};

    fn desc() -> PipelineDesc {
        PipelineDesc {
            topology: PrimitiveTopology::Triangles,
            variant: ShaderVariant::Pbr,
            double_sided: false,
            blend: LayerBlend::Opaque,
            vertex_layout: VertexLayout::standard_3d(),
            requirements: vs_requirements(VS_MAIN).expect("vs_main 必须登记需求"),
        }
    }

    /// 同一组状态必须相等且同哈希（pipeline 缓存 key 的前提）。
    #[test]
    fn test_pipeline_desc_hash_eq() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let h = |d: &PipelineDesc| {
            let mut s = DefaultHasher::new();
            d.hash(&mut s);
            s.finish()
        };
        assert_eq!(desc(), desc());
        assert_eq!(h(&desc()), h(&desc()));

        // 任一维度变化 ⇒ 不相等、哈希不同
        let mut m = desc();
        m.double_sided = true;
        assert_ne!(m, desc());
        assert_ne!(h(&m), h(&desc()));

        let mut m = desc();
        m.blend = LayerBlend::Alpha;
        assert_ne!(h(&m), h(&desc()));

        let mut m = desc();
        m.variant = ShaderVariant::Lambert;
        assert_ne!(h(&m), h(&desc()));

        let mut m = desc();
        m.topology = PrimitiveTopology::Lines;
        assert_ne!(h(&m), h(&desc()));

        // D15：布局是 key 的必要维度（2D / 3D 布局共存）
        let mut m = desc();
        m.vertex_layout = VertexLayout::standard(AaModel::None, CacheStrategy::Static);
        assert_ne!(h(&m), h(&desc()));

        // 需求是 key 的必要维度（两套入口读不同语义时必须落到不同 pipeline）
        let mut m = desc();
        m.requirements = ShaderRequirements {
            semantics: vec![ze_vertex::Semantic::Position],
        };
        assert_ne!(h(&m), h(&desc()));
    }

    // ── 顶点语义需求 ────────────────────────────────────────────────────

    /// `vs_main` 的需求与 3D 标准布局逐项对齐（顺序 = shader location 顺序）。
    ///
    /// 这条断言把"布局声明"和"需求声明"两侧扣在一起：任何一侧调换顺序或增删槽，
    /// 这里立刻红。shader 侧的位置数量由 `wgsl_requirements.rs` 门禁核对。
    #[test]
    fn vs_main_requirements_match_standard_3d_layout() {
        let requirements = vs_requirements(VS_MAIN).expect("vs_main 必须登记");
        let layout = VertexLayout::standard_3d();
        let layout_semantics: Vec<ze_vertex::Semantic> = layout
            .attribs
            .iter()
            .map(|a| ze_vertex::Semantic::of(a.semantic).expect("布局语义必须在词汇表内"))
            .collect();
        assert_eq!(
            requirements.semantics, layout_semantics,
            "vs_main 的需求顺序必须等于 standard_3d() 的属性顺序"
        );
    }

    /// 未登记的入口 ⇒ `None`（调用方显式报错，不猜需求）。
    #[test]
    fn unknown_entry_has_no_requirements() {
        assert_eq!(vs_requirements("vs_nonexistent"), None);
        assert_eq!(vs_requirements(""), None);
    }

    /// 拓扑 → 入口：三角形/点走 `vs_main`，线段走 `vs_line`（只读位置）。
    #[test]
    fn topology_maps_to_its_vertex_entry() {
        assert_eq!(vs_entry_for(PrimitiveTopology::Triangles), VS_MAIN);
        assert_eq!(vs_entry_for(PrimitiveTopology::Points), VS_MAIN);
        assert_eq!(vs_entry_for(PrimitiveTopology::Lines), VS_LINE);

        // 线段需求只有位置 —— 这正是它能把布局缩到 stride 12 的原因。
        assert_eq!(
            vs_requirements_for(PrimitiveTopology::Lines).semantics,
            vec![ze_vertex::Semantic::Position]
        );
        assert_eq!(
            vs_requirements_for(PrimitiveTopology::Triangles).semantics,
            vec![
                ze_vertex::Semantic::Position,
                ze_vertex::Semantic::Uv,
                ze_vertex::Semantic::ShadingNormal,
            ]
        );
    }

    #[test]
    fn test_pass_desc() {
        let p = PassDesc {
            layer: LayerId(3),
            clear_color: ze_notion::Color32::TRANSPARENT,
            depth: PassDepth::TestAndWrite,
        };
        assert_eq!(p.layer, LayerId(3));
        assert_eq!(p.depth, PassDepth::TestAndWrite);
        assert_ne!(PassDepth::None, PassDepth::TestAndWrite);
        assert_eq!(p, p);
    }

    /// D22：能力查询 —— 满足则返回请求值；超限 / 非法 ⇒ 显式错误（不静默降级）。
    #[test]
    fn test_capabilities_require_sample_count() {
        let caps = Capabilities {
            max_sample_count: 4,
            color_format: ColorFormat::Rgba8UnormSrgb,
            depth_format: DepthFormat::Depth32Float,
            post: PostCaps::none(),
        };
        assert_eq!(caps.require_sample_count(1).unwrap(), 1);
        assert_eq!(caps.require_sample_count(4).unwrap(), 4);
        assert_eq!(
            caps.require_sample_count(8),
            Err(CapabilityError::UnsupportedSampleCount {
                requested: 8,
                max_supported: 4
            })
        );
        assert_eq!(
            caps.require_sample_count(3),
            Err(CapabilityError::InvalidSampleCount { requested: 3 })
        );
        assert!(caps.require_sample_count(0).is_err());
    }
}
