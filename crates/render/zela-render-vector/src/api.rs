// SPDX-FileCopyrightText: 2026 Academa, Inc.
// SPDX-License-Identifier: MIT
// Modified by Zela: 新增宿主适配层（B4 Step 5）：把 zela 的 2D 路径喂给矢量后端，
// 离屏渲染一帧并读回 RGBA8。

//! 宿主适配层：`FilledPath` → 上游 `Upload::path` + `Record::filled` → 离屏一帧。
//!
//! 上游按"视图字节 + 记录字节"驱动（Python 侧负责打包）。本模块提供**同一形态**的
//! Rust 入口，供 zela 宿主使用：视图用上游 `feed.py` 的 2D 正交布局（见
//! `view_2d`），记录用 `Record::filled`（单填充路径）。
//!
//! 已知非等价（B4 计划 §5.4）：上游的 `m1/m2` 双变换 + GPU 插值。
//! **PH Step 1** 起 `m2` 已用于形变（`m2 = mix · I` + 差值形状），其余记录仍只写 `m1`。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::render::{Record, Upload, VIEW_LENGTH, VIEW_LIGHTING, VIEW_OCCLUSION};

// ─── PH Step 3：自定义环境（宿主上传） ─────────────────────────────────────

/// 一条已上传的环境（RGBE 字节；`Arc` 让每次装进 `Player` 不必再复制一份）。
struct EnvironmentUpload {
    id: u32,
    width: u32,
    height: u32,
    rgbe: Arc<Vec<u8>>,
}

/// 宿主上传的环境表（进程级；每个 `Player` 在代数变化时各装一份 —— 预过滤立方体是
/// **每 player 的 GPU 资源**，不能共享）。
static UPLOADED_ENVIRONMENTS: OnceLock<Mutex<Vec<EnvironmentUpload>>> = OnceLock::new();

/// 上传代数：任一环境变化 +1。`Player` 按它决定是否重装（同代完全不碰）。
static ENVIRONMENT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// **PH Step 3**：上传一张自定义环境图（Radiance RGBE 字节），供 `Light::Environment { id }` 引用。
///
/// `rgbe` 是 `width × height × 4` 字节（行优先、自上而下；用 [`crate::read_hdr`] 解
/// `.hdr` 文件）。**id 0 保留给内置影棚**（[`crate::STUDIO`] / [`crate::BUILTIN_ENVIRONMENT`]）。
///
/// 语义：同 id 再次上传 = **替换**；上传后下一次渲染自动生效（代数变化 ⇒ 每个常驻
/// player 重装一次，不会每帧重算 harmonics）。
pub fn add_environment(id: u32, width: u32, height: u32, rgbe: &[u8]) -> Result<(), String> {
    if id == crate::environment::STUDIO {
        return Err(format!(
            "环境 id {id} 是内置影棚的保留 id（STUDIO）；自定义环境请用非 0 id"
        ));
    }
    if width == 0 || height == 0 {
        return Err(format!("环境尺寸不能为 0（{width}×{height}）"));
    }
    let needed = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| format!("环境尺寸过大：{width}×{height}"))?;
    if rgbe.len() != needed {
        return Err(format!(
            "环境字节数 {} ≠ {width}×{height}×4 = {needed}",
            rgbe.len()
        ));
    }
    let upload = EnvironmentUpload {
        id,
        width,
        height,
        rgbe: Arc::new(rgbe.to_vec()),
    };
    let mut list = UPLOADED_ENVIRONMENTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match list.iter_mut().find(|entry| entry.id == id) {
        Some(slot) => *slot = upload,
        None => list.push(upload),
    }
    ENVIRONMENT_GENERATION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// 把宿主上传的环境装进当前 player（代数相同则什么都不做）。
///
/// 为什么按代数而不是"有就跳过"：同 id 再上传是一次**替换** —— 只比"有没有"会让
/// 新图永远进不来（GAPS PC-05 的同型：只比存在、不比是哪一个）。
fn install_environments(player: &mut crate::render::Player) -> Result<(), String> {
    let generation = ENVIRONMENT_GENERATION.load(Ordering::Relaxed);
    if player.environment_generation() == generation {
        return Ok(());
    }
    let list = UPLOADED_ENVIRONMENTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for environment in list.iter() {
        player.add_environment(
            environment.id,
            environment.width,
            environment.height,
            &environment.rgbe,
        )?;
    }
    drop(list);
    player.set_environment_generation(generation);
    Ok(())
}

/// 一条填充路径（世界坐标；曲线为每段 4 个控制点）。
#[derive(Debug, Clone)]
pub struct FilledPath {
    /// 控制点：每 4 个一组（一段三次贝塞尔；直线段用四等分点）。
    pub points: Vec<[f64; 3]>,
    /// 子路径区间：`[首曲线, 末曲线, 是否闭合, 0]`。
    pub ranges: Vec<[u32; 4]>,
    /// 填充色（straight RGBA，0..1）。
    pub fill: [f32; 4],
    /// 3×4 模型矩阵（行主序；2D 视图里这就是"对象 → 世界"）。
    pub model: [[f32; 4]; 3],
    /// **B6 Step 1.2**：3D 视图里按光照着色 —— 上游 `feed.py:1134` 的 `shade_in_3d`
    /// （`flags |= LIT`）。2D 视图里无意义（上游同样只在 `three_d` 时置位）。
    pub shade_in_3d: bool,
    /// **B6 Step 1.2**：材质 `[metallic, roughness, reflectance]` —— 上游 `feed.py:1150`
    /// （`material = (m, r, f, 1)`）。`Some(_)` ⇒ 该平面路径按 PBR / 环境光受光；
    /// `None` ⇒ 上游的 `NO_MATERIAL`（`material[3] = 0`）。
    pub material: Option<[f32; 3]>,
    /// **PD**：可选的后端已实现参数（默认 = 当前行为）。
    pub options: PathOptions,
    /// **PH Step 1 / 参数化几何**：逐帧只变数字的那一半（`None` = 普通路径）。
    ///
    /// 阶段 1 起三种几何动画共用同一 idiom（[`ParamGeometry`]）：
    /// `Lerp`（形变）/ `Window`（部分显示）/ `Grow`（按曲线数生长）。
    pub param: Option<ParamGeometry>,
    /// **阶段 2（着色 codegen）**：填充色由一段生成的 WGSL 在 GPU 上算
    /// （`None` = 普通填充）。装饰性：不影响几何 / 状态，也不进结构指纹。
    pub shader: Option<crate::render::ShaderFill>,
}

/// 参数化几何的种类（阶段 1）。
///
/// 三者的共同点才是这个类型的意义：**结构（控制点）与逐帧参数分离**。
/// 作者眼里 `Morph` / `Reveal` / `Grow` 都是"几何上传一次，每帧只换几个数"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// 两个同长形状之间按 `mix` 线性插值（`PH` 的形变）。
    Lerp,
    /// 只显示曲线索引窗口 `[lo, hi]`（`PE` 的 `curve_window`）。
    Window,
    /// 只显示前 `curves` 条曲线 —— `Window` 的特例（`[0, curves]`）。
    Grow,
}

/// 一条参数化几何（阶段 1）：`kind` 决定 `params` 的语义，`delta` 只在 `Lerp` 时有内容。
///
/// 后端语义不变：`Lerp` 走第二形状（`curve_h` 逐曲线相加，`mix` 烤进 `m2`）；
/// `Window` / `Grow` 走 `params[0..2]` 的曲线窗口（`vector.wgsl` 的 `params_of`）。
/// 本类型只负责把三种写法收成**一份**公开面（`param_records`），
/// 以及给结构指纹一个"只哈希参数、不哈希求值结果"的入口（[`ParamGeometry::param_digest`]）。
#[derive(Debug, Clone, PartialEq)]
pub struct ParamGeometry {
    /// 参数语义（同时是 digest 的域标签）。
    pub kind: ParamKind,
    /// `Lerp` 的差值控制点（`to − from`，与首形状同长；每段 4 个）。其它 kind 为空。
    pub delta: Vec<[f64; 3]>,
    /// 定长 f32 参数：
    /// - `Lerp`：`[mix]`
    /// - `Window`：`[lo, hi]`（曲线索引 + t，`hi > lo`）
    /// - `Grow`：`[curves]`（可见曲线数 ≥ 0）
    pub params: Vec<f32>,
}

impl ParamGeometry {
    /// 形变（`Lerp`）：`delta = to − from`，`mix ∈ [0, 1]`。
    pub fn lerp(delta: Vec<[f64; 3]>, mix: f32) -> Self {
        Self {
            kind: ParamKind::Lerp,
            delta,
            params: vec![mix],
        }
    }

    /// 曲线窗口（`Window`）：单位是**曲线索引 + t**，与后端 `params[0..2]` 同口径。
    pub fn window(lo: f32, hi: f32) -> Self {
        Self {
            kind: ParamKind::Window,
            delta: Vec::new(),
            params: vec![lo, hi],
        }
    }

    /// 生长（`Grow`）：显示前 `curves` 条曲线（闭合形状的 0 条 = 不可见）。
    pub fn grow(curves: f32) -> Self {
        Self {
            kind: ParamKind::Grow,
            delta: Vec::new(),
            params: vec![curves],
        }
    }

    /// `Lerp` 的混合权重（其它 kind 为 `None`）。
    pub fn mix(&self) -> Option<f32> {
        (self.kind == ParamKind::Lerp).then(|| self.params[0])
    }

    /// `Window` / `Grow` 对应的曲线窗口 `[lo, hi]`（`Lerp` 为 `None`）。
    pub fn window_range(&self) -> Option<[f32; 2]> {
        match (self.kind, self.params.as_slice()) {
            (ParamKind::Window, [lo, hi]) => Some([*lo, *hi]),
            (ParamKind::Grow, [curves]) => Some([0.0, *curves]),
            _ => None,
        }
    }

    /// **结构指纹口径**：只哈希 `kind + params`，**不哈希求值结果**（也不哈希 `delta` 的坐标）。
    ///
    /// 为什么这样切：几何是派生量（`Path` 对结构指纹失明，见 GAPS AR-05/AR-08）；
    /// 逐帧真正变化的只有参数。`delta` / `base` 属于"结构上传一次"的一半，由记录本身的
    /// 结构键负责，不进每帧参数摘要 ⇒ 同一份参数在两次求值后 digest 逐位相同。
    pub fn param_digest(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut write = |byte: u8| {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };
        write(match self.kind {
            ParamKind::Lerp => b'l',
            ParamKind::Window => b'w',
            ParamKind::Grow => b'g',
        });
        for param in &self.params {
            for byte in param.to_bits().to_le_bytes() {
                write(byte);
            }
        }
        hash
    }
}


/// **PE Step 1**：绘制参数类型的唯一来源在 `ze-render-ir`（场景层与后端共用一份）。
///
/// 它们原先定义在本 crate 里 ⇒ 场景层（不依赖本 crate）**无法引用**，能力没有入口。
pub use ze_render_ir::{Dash, Gradient, PathOptions, SecondPaint};

impl FilledPath {
    /// 由折线构造闭合填充路径（每段转成一条直线三次曲线）。
    pub fn polygon(points: &[[f64; 3]], fill: [f32; 4]) -> Self {
        let n = points.len();
        let mut curves = Vec::with_capacity(n * 4);
        for i in 0..n {
            let a = points[i];
            let b = points[(i + 1) % n];
            let lerp = |t: f64| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
            curves.extend_from_slice(&[a, lerp(1.0 / 3.0), lerp(2.0 / 3.0), b]);
        }
        Self {
            points: curves,
            ranges: vec![[0, n as u32, 1, 0]],
            fill,
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            shade_in_3d: false,
            material: None,
            options: PathOptions::default(),
            param: None,
            shader: None,
        }
    }
}

/// 2D 视图的**着色栈选项**（B6 Step 1.2）：默认全关 ⇒ 与 B4 的 `view_2d` 逐位相同。
///
/// 槽位见 `render.rs` 的 `VIEW_LIGHTING` / `VIEW_OCCLUSION`：
/// `+5` 曝光、`+6` tone（0 = clip / 1 = AgX）、`+7` bloom 强度、
/// `VIEW_OCCLUSION +0/+1` = AO 强度 / 半径。
///
/// ⚠️ **AO 依赖环境光**（`B6 §2.4`）：上游 `occlusion_of()` 要求场景里存在
/// 环境光（类型 4）或全向光（类型 0）；只有方向光时 AO **静默不生效**。
/// 本入口的 2D 视图没有灯 ⇒ 打开 AO 不会改变像素（这是上游语义，不是没接上）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    pub exposure: f32,
    /// `Some(curve)` ⇒ 打开 tone 映射（`Clip` = 0、`AgX` = 1，与 `tone.wgsl` 一致）。
    pub tone: Option<ze_render_ir::ToneCurve>,
    pub bloom_strength: f32,
    pub ao: Option<(f32, f32)>,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            tone: None,
            bloom_strength: 0.0,
            ao: None,
        }
    }
}

/// 2D 正交视图（与上游 `feed.py::view` 的 2D 分支同布局）：
/// 世界坐标 = 以画面中心为原点的像素。
fn view_2d_with(width: u32, height: u32, background: [f32; 4], options: ViewOptions) -> Vec<f32> {
    let (hw, hh) = (width as f32 * 0.5, height as f32 * 0.5);
    let mut v = vec![0.0f32; VIEW_LENGTH];
    let projection = [
        [1.0 / hw, 0.0, 0.0, 0.0],
        [0.0, 1.0 / hh, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.5],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let overlay = [
        [1.0 / hw, 0.0, 0.0, 0.0],
        [0.0, 1.0 / hh, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for c in 0..4 {
        for r in 0..4 {
            v[c * 4 + r] = projection[r][c];
            v[16 + c * 4 + r] = overlay[r][c];
        }
    }
    v[32] = width as f32;
    v[33] = height as f32;
    v[34] = 1.0; // unit：像素 / 场景单位（frame_height = height）
    v[35] = 1.0; // zoom
    v[39] = 0.0; // 2D
    v[42] = 1.0; // toward = +z
    v[44..48].copy_from_slice(&background);
    v[52] = 0.0; // 2D 视图没有光源（AO 因此不会生效，见 `ViewOptions`）
    v[53] = options.exposure;
    // 着色栈槽位：与渲染器共用同一组偏移常量（避免两份布局漂移）。
    let tone = match options.tone {
        None => 0.0,
        Some(ze_render_ir::ToneCurve::Clip) => 0.0,
        Some(ze_render_ir::ToneCurve::AgX) => 1.0,
    };
    v[VIEW_LIGHTING + 6] = tone;
    v[VIEW_LIGHTING + 7] = options.bloom_strength.clamp(0.0, 1.0);
    if let Some((intensity, radius)) = options.ao {
        v[VIEW_OCCLUSION] = intensity.max(0.0);
        v[VIEW_OCCLUSION + 1] = radius.max(0.0);
    }
    v
}

/// 控制点的质心与新厄尔面积向量（上游 `Upload::path` 的第六个参数）。
fn centroid_area(points: &[[f64; 3]]) -> [f32; 6] {
    let n = points.len().max(1) as f64;
    let centroid = [0, 1, 2].map(|k| (points.iter().map(|p| p[k]).sum::<f64>() / n) as f32);
    let mut area = [0.0f64; 3];
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        area[0] += (a[1] - b[1]) * (a[2] + b[2]);
        area[1] += (a[2] - b[2]) * (a[0] + b[0]);
        area[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    [
        centroid[0],
        centroid[1],
        centroid[2],
        area[0] as f32 * 0.5,
        area[1] as f32 * 0.5,
        area[2] as f32 * 0.5,
    ]
}

/// 一条 **3D 网格**记录（B10 Step 2）：模型空间顶点 + 三角形索引 + 材质。
///
/// 输入精度与上游一致：位置 / UV / 法线都是 **f64**，三角形是 u32。上游
/// `Mesh::new` 要求"每个点一个 (u, v)" ⇒ [`uvs`](Self::uvs) 为 `None` 时本入口补零；
/// `normals` 为 `None` 时由上游按面积加权自动求法线（`smooth_normals`）。
///
/// 本批**不带纹理**（GAPS V2-13：纹理 / 渐变的宿主入口仍缺 —— 渐变已在 PE 接通，
/// 纹理仍属 PD-03 顺延 PF 的那一项）。
#[derive(Debug, Clone, PartialEq)]
pub struct MeshRecord {
    /// 顶点位置（模型空间）。
    pub positions: Vec<[f64; 3]>,
    /// 可选逐顶点 UV（长度须等于 `positions`；`None` ⇒ 上游补 `(0, 0)`）。
    pub uvs: Option<Vec<[f64; 2]>>,
    /// 可选逐顶点法线（长度须等于 `positions`；`None` ⇒ 上游按面积加权求）。
    pub normals: Option<Vec<[f64; 3]>>,
    /// 三角形索引（三个一组；`positions` 的下标）。
    pub triangles: Vec<u32>,
    /// 表面色（straight RGBA，0..1）；受光时作为反照率。
    pub fill: [f32; 4],
    /// 3×4 模型矩阵（行主序）。
    pub model: [[f32; 4]; 3],
    /// 材质 `[metallic, roughness, reflectance]`；`Some(_)` ⇒ 该网格按材质逐片元受光
    /// （上游 `material[3] = 1`）。`None` ⇒ 不受光（只按 `fill` 平涂）。
    pub material: Option<[f32; 3]>,
    /// **PD**：逐顶点色（长度须等于 `positions`；`None` ⇒ 用 `fill`）。
    /// 上游编码是 brush rows（`vertex_color` 按顶点索引取行）。
    pub colors: Option<Vec<[f32; 4]>>,
    /// **PD**：第二套表面色与混合进度（tween；只用到其中的 `fill` 与 `mix`）。
    pub second: Option<SecondPaint>,
    /// **PG Step 2/4**：纹理来源（`None` = 无纹理）。纹理只对网格生效
    /// （`render.rs` 的 mesh 分支绑定 `image_groups`）；路径记录没有采样器入口。
    pub texture: Option<TextureSource>,
}

/// 网格的纹理来源（PG Step 2/4）。
///
/// 两条来源落在后端**同一张采样表**里（`render.rs` 的 `image_groups`）：相机视图的 canvas
/// 与上传的 RGBA 图像按同一个 `uv` 采样，所以公开面只需要一个"网格带纹理"的字段。
#[derive(Debug, Clone, PartialEq)]
pub enum TextureSource {
    /// 同一帧里**更早**的相机视图（[`CameraView`] 的 `key`）；找不到 ⇒ 显式 `unknown texture`。
    View(u64),
    /// 直接带一张 RGBA8 图像（行优先；`width * height * 4` 字节）。
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

impl MeshRecord {
    /// 几何自洽性检查（显式失败，不静默丢三角形）。
    pub fn validate(&self) -> Result<(), String> {
        if self.positions.is_empty() {
            return Err("mesh: 顶点为空".to_string());
        }
        if self.triangles.is_empty() || self.triangles.len() % 3 != 0 {
            return Err(format!(
                "mesh: 三角形索引必须是三个一组（实际 {} 个）",
                self.triangles.len()
            ));
        }
        if let Some(index) = self
            .triangles
            .iter()
            .find(|&&index| index as usize >= self.positions.len())
        {
            return Err(format!(
                "mesh: 索引 {index} 超出顶点数 {}",
                self.positions.len()
            ));
        }
        if let Some(uvs) = &self.uvs {
            if uvs.len() != self.positions.len() {
                return Err(format!(
                    "mesh: UV 数 {} ≠ 顶点数 {}",
                    uvs.len(),
                    self.positions.len()
                ));
            }
        }
        if let Some(normals) = &self.normals {
            if normals.len() != self.positions.len() {
                return Err(format!(
                    "mesh: 法线数 {} ≠ 顶点数 {}",
                    normals.len(),
                    self.positions.len()
                ));
            }
        }
        if let Some(colors) = &self.colors {
            if colors.len() != self.positions.len() {
                return Err(format!(
                    "mesh: 逐顶点色 {} ≠ 顶点数 {}",
                    colors.len(),
                    self.positions.len()
                ));
            }
        }
        Ok(())
    }
}

/// 一条**描边**路径（`Record` 的描边形态，B6 Step 2.1）。
///
/// 上游编码：`params.z` = 描边**宽度**（`Record::stroked` 原样写入，不缩放、不减半）；
/// `Paint.brush2.w = cap | joint << 2`（`0` = butt + miter）。
/// miter limit 在着色器里硬编码 10（GAPS V2-11），不由记录传入。
///
/// ⚠️ 着色器把该槽位读进名为 `half` 的局部量（`vector.wgsl:807`），那是**它内部的**
/// 命名，不是本字段的口径。口径以实测钉住：`stroke_width_is_the_drawn_width`
/// 断言"传 10 px 量到 10 px"（B11 §10）。
#[derive(Debug, Clone)]
pub struct StrokedPath {
    /// 控制点：每 4 个一组（与 [`FilledPath`] 相同口径）。
    pub points: Vec<[f64; 3]>,
    /// 子路径区间：`[首曲线, 末曲线, 是否闭合, 0]`。
    pub ranges: Vec<[u32; 4]>,
    /// 描边色（straight RGBA，0..1）。
    pub stroke: [f32; 4],
    /// 描边**宽度**（场景单位，`camera.unit` 换算为像素；**原样**写入 `params.z`）。
    pub width: f32,
    /// 形态位：`cap | joint << 2`（0 = butt + miter）。
    pub shape_bits: u32,
    pub model: [[f32; 4]; 3],
    /// **PD**：可选的后端已实现参数（默认 = 当前行为）。
    pub options: PathOptions,
    /// **PH Step 1 / 参数化几何**：逐帧只变数字的那一半（`None` = 普通描边）。
    pub param: Option<ParamGeometry>,
}

/// 一盏灯（B6 Step 1.2）：编码移植自上游 `rendering/feed.py::lighting`，
/// 每盏 16 float：`位置/方向 xyz`、`kind`、`线性色 rgb × 强度`、`reach`、
/// `轴线 xyz`、`scale`、`offset`、`是否投影`、0、0。
///
/// `kind`：0 = 环境光、1 = 平行光、2 = 点光、3 = 聚光、4 = 环境图（与上游一致）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Light {
    Ambient {
        color: [f32; 3],
        intensity: f32,
    },
    Sun {
        /// 光来自的方向（世界坐标；上游会归一化）。
        direction: [f32; 3],
        color: [f32; 3],
        intensity: f32,
        shadows: bool,
    },
    Point {
        at: [f32; 3],
        color: [f32; 3],
        intensity: f32,
        /// 影响半径（0 = 无衰减）。
        reach: f32,
    },
    Spot {
        at: [f32; 3],
        toward: [f32; 3],
        color: [f32; 3],
        intensity: f32,
        /// 半角（弧度）与软化比例，与上游 `angle` / `softness` 同义。
        angle: f32,
        softness: f32,
        shadows: bool,
    },
    /// **B10 Step 2**：环境图（上游 `kind = 4`）。`id` 指向 [`Player`] 里
    /// `add_environment` 上传的那张图；上游把它放在"点光 reach"的槽位（`slot[7]`）。
    ///
    /// AO 的生效条件正是"存在环境光（kind 0）或环境图（kind 4）"（见 `occlusion_of`）。
    Environment {
        id: u32,
    },
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / n, v[1] / n, v[2] / n]
}

/// sRGB 编码值 → 线性光（上游 `feed.py::linear`）。
fn linear(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    })
}

impl Light {
    fn slot(&self) -> [f32; 16] {
        let mut out = [0.0f32; 16];
        match *self {
            Light::Ambient { color, intensity } => {
                out[3] = 0.0;
                let c = linear(color).map(|c| c * intensity);
                out[4..7].copy_from_slice(&c);
            }
            Light::Sun {
                direction,
                color,
                intensity,
                shadows,
            } => {
                out[0..3].copy_from_slice(&normalize3(direction));
                out[3] = 1.0;
                let c = linear(color).map(|c| c * intensity);
                out[4..7].copy_from_slice(&c);
                out[13] = if shadows { 1.0 } else { 0.0 };
            }
            Light::Point {
                at,
                color,
                intensity,
                reach,
            } => {
                out[0..3].copy_from_slice(&at);
                out[3] = 2.0;
                let c = linear(color).map(|c| c * intensity);
                out[4..7].copy_from_slice(&c);
                out[7] = reach;
            }
            Light::Spot {
                at,
                toward,
                color,
                intensity,
                angle,
                softness,
                shadows,
            } => {
                out[0..3].copy_from_slice(&at);
                out[3] = 3.0;
                let c = linear(color).map(|c| c * intensity);
                out[4..7].copy_from_slice(&c);
                let axis = normalize3([toward[0] - at[0], toward[1] - at[1], toward[2] - at[2]]);
                out[8..11].copy_from_slice(&axis);
                let outer = angle.cos();
                let inner = (angle * (1.0 - softness)).cos();
                let scale = 1.0 / (inner - outer).max(1e-4);
                out[11] = scale;
                out[12] = -outer * scale;
                out[13] = if shadows { 1.0 } else { 0.0 };
            }
            // B10 Step 2：环境图槽位 —— `kind = 4`。
            //
            // **PG Step 3**：此前只写了 kind 与 id，`color.rgb` / `axis.w`（立方体里 1 代表的
            // 光强）全是 0 ⇒ `light.wgsl` 的 kind-4 分支把整项乘成 0，**IBL 永远不发光**
            // （这也是"接了 STUDIO 画面逐位不变"的真因）。这里补齐成上游的完整编码：
            // - `place.xyz` = 图的 +x（世界 +x），`axis.xyz` = 图的 up（世界 +z）⇒ 方向基准；
            // - `color.rgb` = 白色环境光（1 = 单位光强），`color.w` = 立方体最粗一级；
            // - `axis.w` = 立方体里 1 代表的光强（`Environment::unit`）；
            // - `cone.x`（槽 12）= **id**：上游把它塞在"点光 reach"槽（`color.w`），
            //   而着色器把 `color.w` 读作最粗一级 —— 两者不能同槽。
            Light::Environment { id } => {
                out[3] = 4.0;
                out[0] = 1.0;
                out[8] = 0.0;
                out[9] = 0.0;
                out[10] = 1.0;
                out[4] = 1.0;
                out[5] = 1.0;
                out[6] = 1.0;
                out[7] = (crate::environment::LEVELS - 1) as f32;
                out[11] = 1.0;
                out[12] = id as f32;
            }
        }
        out
    }
}

/// 一台 3D 相机视图（B6 Step 1.2）：移植自上游 `feed.py::view` 的 3D 分支。
///
/// `axes` 是**世界 → 相机**的 3×3（行 = 相机右 / 上 / 朝外）。宿主已有 `Camera3D`，
/// 由它算出三个轴即可；本 crate 不做轨道角（phi/theta/gamma）的约定。
#[derive(Debug, Clone, PartialEq)]
pub struct View3 {
    /// 相机画幅（世界单位）。
    pub frame: (f32, f32),
    /// 画幅中心（世界坐标）。
    pub center: [f32; 3],
    /// 世界 → 相机轴（行主序）。
    pub axes: [[f32; 3]; 3],
    pub zoom: f32,
    /// 焦距（眼到画幅的距离）。
    pub focal: f32,
    pub size: (u32, u32),
    pub background: [f32; 4],
    /// 默认光的方向（上游 `camera.light_source`；本入口只在 `lights` 为空时用于记录）。
    pub light_source: [f32; 3],
    pub exposure: f32,
    pub tone: Option<ze_render_ir::ToneCurve>,
    pub bloom: f32,
    /// **PB Step 2（V-02）**：泛光的亮度门槛（低于它不参与扩散）。0 = 不设门槛（旧行为）。
    pub bloom_threshold: f32,
    /// **PB Step 2（V-02）**：泛光半径（屏幕像素），决定金字塔最粗一级；0 = 用满 `LEVELS`。
    pub bloom_radius: f32,
    /// ⚠️ **AO 依赖环境光**（`B6 §2.4`）：上游 `occlusion_of()` 要求 `lights` 里存在
    /// 环境光（类型 0）或环境图（类型 4）；只有方向光 / 点光 / 聚光时 **静默不生效**。
    pub ao: Option<(f32, f32)>,
    pub lights: Vec<Light>,
}

/// 上游常数：近平面（reversed Z 的基准）与"贴在表面上"的深度偏置。
const NEAR: f32 = 0.05;
const ON_3D: f32 = 1e-6;

fn view_3d(v: &View3) -> Vec<f32> {
    let (hw, hh) = (v.frame.0 * 0.5, v.frame.1 * 0.5);
    let [rx, ry, rz] = v.axes;
    let r = [rx, ry, rz];
    let rc = [0, 1, 2].map(|k| rx[k] * v.center[0] + ry[k] * v.center[1] + rz[k] * v.center[2]);
    let f = v.focal.max(1e-6);
    let zoom = v.zoom;
    // 上游的 projection（行主序，`feed.py:438`），再按 WGSL 需要转置写入。
    let projection = [
        [
            zoom / hw * r[0][0],
            zoom / hw * r[0][1],
            zoom / hw * r[0][2],
            -zoom / hw * rc[0],
        ],
        [
            zoom / hh * r[1][0],
            zoom / hh * r[1][1],
            zoom / hh * r[1][2],
            -zoom / hh * rc[1],
        ],
        [0.0, 0.0, 0.0, NEAR / f],
        [-r[2][0] / f, -r[2][1] / f, -r[2][2] / f, (f + rc[2]) / f],
    ];
    let overlay = [
        [1.0 / hw, 0.0, 0.0, 0.0],
        [0.0, 1.0 / hh, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut out = vec![0.0f32; VIEW_LENGTH];
    for c in 0..4 {
        for row in 0..4 {
            out[c * 4 + row] = projection[row][c];
            out[16 + c * 4 + row] = overlay[row][c];
        }
    }
    out[32] = v.size.0 as f32;
    out[33] = v.size.1 as f32;
    out[34] = v.size.1 as f32 / v.frame.1;
    out[35] = zoom;
    out[36..39].copy_from_slice(&v.light_source);
    out[39] = 1.0; // three_d
    // `toward` = 相机 z 轴（世界），bias = ON_3D。
    out[40..43].copy_from_slice(&rz);
    out[43] = ON_3D;
    out[44..48].copy_from_slice(&v.background);
    let eye = [
        v.center[0] + f * rz[0],
        v.center[1] + f * rz[1],
        v.center[2] + f * rz[2],
        1.0,
    ];
    out[VIEW_LIGHTING..VIEW_LIGHTING + 4].copy_from_slice(&eye);
    let count = v.lights.len().min(8);
    out[VIEW_LIGHTING + 4] = count as f32;
    out[VIEW_LIGHTING + 5] = v.exposure;
    out[VIEW_LIGHTING + 6] = match v.tone {
        None | Some(ze_render_ir::ToneCurve::Clip) => 0.0,
        Some(ze_render_ir::ToneCurve::AgX) => 1.0,
    };
    out[VIEW_LIGHTING + 7] = v.bloom.clamp(0.0, 1.0);
    for (i, light) in v.lights.iter().take(8).enumerate() {
        let at = VIEW_LIGHTING + 8 + 16 * i;
        out[at..at + 16].copy_from_slice(&light.slot());
    }
    if let Some((intensity, radius)) = v.ao {
        out[VIEW_OCCLUSION] = intensity.max(0.0);
        out[VIEW_OCCLUSION + 1] = radius.max(0.0);
    }
    // PB Step 2（V-02）：`occlusion.z/.w` 是视图里仅剩的两个空槽（着色器只读 `.x/.y`）。
    // 不要再往 `VIEW_LIGHTING` 里挤 —— 那 8 个槽（eye / 灯数 / 曝光 / tone / bloom）已满。
    out[VIEW_OCCLUSION + 2] = v.bloom_threshold.max(0.0);
    out[VIEW_OCCLUSION + 3] = v.bloom_radius.max(0.0);
    out
}

/// 一帧的记录字节（路径已上传进 player；view 与 records 供 `views` 用）。
struct Prepared {
    view: Vec<u8>,
    records: Vec<u8>,
    /// **阶段 2（着色 codegen）**：`(记录索引, 程序索引)` —— 由 compute 在光栅前写填充槽。
    fills: Vec<(u32, u32)>,
    programs: Vec<crate::render::ShaderFill>,
}

/// brush rows 的键空间（与路径 / 网格的键 `1..=n` 分开；`Store` 的 `brushes` 是独立映射）。
const ROW_KEY_BASE: u64 = 1 << 40;
/// **PG Step 4**：内联图像纹理的键空间（与 rows `1<<40`、相机视图键 `1<<32…` 都分开）。
const IMAGE_KEY_BASE: u64 = 1 << 41;

/// 把 [`PathOptions`] 写进一条记录：**每个字段都对应 `Record` 里一个已实现的槽位**。
///
/// `fill` = 这条记录是填充（渐变写 `fill_rows`）还是描边（写 `stroke_rows`）。
fn apply_options(
    record: &mut Record,
    options: &PathOptions,
    player: &mut crate::render::Player,
    key: u64,
    fill: bool,
) -> Result<(), String> {
    // PF Step 2：屏幕空间叠加 —— 与 PC 的「图层级」相同性质：组合方式必须可观测，
    // 但它不改记录内容，只改投影选择（`vector.wgsl` 读 OVERLAY 位）。
    if options.overlay {
        record.flags |= crate::render::OVERLAY;
    }
    if let Some(background) = options.background {
        record.background = background;
        record.params[3] = options.background_width.max(0.0);
    }
    if let Some(second) = options.second {
        record.fill2 = second.fill;
        record.stroke2 = second.stroke;
        record.background2 = second.background;
        record.dash[3] = second.mix.clamp(0.0, 1.0);
    }
    if let Some(dash) = options.dash {
        if !(dash.period > 0.0) {
            return Err("dash.period 必须 > 0（不画虚线就不要给 dash）".to_string());
        }
        record.dash[0] = dash.period;
        record.dash[1] = dash.duty.clamp(0.0, 1.0);
        record.dash[2] = dash.phase;
    }
    if let Some([lo, hi]) = options.curve_window {
        if !(hi > lo) {
            return Err(format!("curve_window 必须 hi > lo（收到 [{lo}, {hi}]）"));
        }
        record.params[0] = lo;
        record.params[1] = hi;
    }
    if let Some(gradient) = &options.gradient {
        if gradient.stops.len() < 2 {
            return Err(format!(
                "gradient 至少两个色标（收到 {}）",
                gradient.stops.len()
            ));
        }
        let row_key = ROW_KEY_BASE | key;
        player.add_rows(row_key, bytemuck::cast_slice(&gradient.stops))?;
        if fill {
            record.fill_rows = row_key;
        } else {
            record.stroke_rows = row_key;
        }
        record.gradient_a = [gradient.from[0], gradient.from[1], gradient.from[2], 0.0];
        record.gradient_b = [gradient.to[0], gradient.to[1], gradient.to[2], 0.0];
    }
    Ok(())
}

/// 把一帧的路径 / 网格上传进**常驻 player** 并打包记录：同步 / 异步两条渲染路径共用这一份，
/// 避免两套实现在几何映射上漂移（B15）；player 由 [`crate::render::Gpu::take_player`] 复用（B17）。
///
/// `key_base`：记录键的基址（主帧恒 0 ⇒ 键 1..=n；各相机视图各用一段，
/// 否则同一个 `key` 在两份记录里指向同一条 shape，后上传的会覆盖先前的）。
fn prepare_view(
    player: &mut crate::render::Player,
    view: Vec<f32>,
    width: u32,
    height: u32,
    records: &[PathRecord],
    key_base: u64,
) -> Result<Prepared, String> {
    if width == 0 || height == 0 {
        return Err(format!("canvas {width}x{height} is empty"));
    }
    // **PH Step 3**：宿主上传的环境先装进 player（代数相同 ⇒ 空操作）。
    install_environments(player)?;
    let view_bytes: Vec<u8> = bytemuck::cast_slice(&view).to_vec();

    let mut built: Vec<Record> = Vec::with_capacity(records.len());
    // **阶段 2**：着色 codegen 的程序表（同一 (source, params) 只收一次）与记录→程序映射。
    let mut fills: Vec<(u32, u32)> = Vec::new();
    let mut programs: Vec<crate::render::ShaderFill> = Vec::new();
    for (i, record) in records.iter().enumerate() {
        let key = key_base + i as u64 + 1;
        match record {
            PathRecord::Fill(path) => {
                let curves = upload_path(player, key, i, &path.points, &path.ranges)?;
                let mut record = Record::filled(
                    key,
                    curves,
                    path.model,
                    path.fill,
                    path.shade_in_3d,
                    path.material,
                );
                if let Some(shader) = &path.shader {
                    let index = match programs.iter().position(|p| p == shader) {
                        Some(index) => index as u32,
                        None => {
                            programs.push(shader.clone());
                            (programs.len() - 1) as u32
                        }
                    };
                    fills.push((i as u32, index));
                }
                apply_options(&mut record, &path.options, player, key, true)?;
                record = attach_param(
                    player,
                    record,
                    path.param.as_ref(),
                    &path.options,
                    &path.points,
                    &path.ranges,
                    i,
                    records.len(),
                    key_base,
                )?;
                built.push(record);
            }
            PathRecord::Stroke(path) => {
                let curves = upload_path(player, key, i, &path.points, &path.ranges)?;
                let mut record = Record::stroked(
                    key,
                    curves,
                    path.model,
                    path.stroke,
                    path.width,
                    path.shape_bits,
                );
                apply_options(&mut record, &path.options, player, key, false)?;
                record = attach_param(
                    player,
                    record,
                    path.param.as_ref(),
                    &path.options,
                    &path.points,
                    &path.ranges,
                    i,
                    records.len(),
                    key_base,
                )?;
                built.push(record);
            }
            PathRecord::Mesh(mesh) => {
                mesh.validate()
                    .map_err(|error| format!("record {i}: {error}"))?;
                // 上游要求"每个点一个 (u, v)"：没有 UV 时补零（不改变几何）。
                let zero_uvs;
                let uvs: &[[f64; 2]] = match &mesh.uvs {
                    Some(uvs) => uvs,
                    None => {
                        zero_uvs = vec![[0.0, 0.0]; mesh.positions.len()];
                        &zero_uvs
                    }
                };
                let point_bytes: &[u8] = bytemuck::cast_slice(&mesh.positions);
                let uv_bytes: &[u8] = bytemuck::cast_slice(uvs);
                let normal_bytes: &[u8] = match &mesh.normals {
                    Some(normals) => bytemuck::cast_slice(normals),
                    None => &[],
                };
                let triangle_bytes: &[u8] = bytemuck::cast_slice(&mesh.triangles);
                player.upload(Upload::mesh(
                    key,
                    point_bytes,
                    uv_bytes,
                    normal_bytes,
                    triangle_bytes,
                    0,
                    0,
                )?)?;
                let mut record = Record::mesh(
                    key,
                    (mesh.triangles.len() / 3) as u32,
                    mesh.model,
                    mesh.fill,
                    mesh.material,
                );
                if let Some(colors) = &mesh.colors {
                    let row_key = ROW_KEY_BASE | key;
                    player.add_rows(row_key, bytemuck::cast_slice(colors))?;
                    record.fill_rows = row_key;
                }
                if let Some(second) = mesh.second {
                    record.fill2 = second.fill;
                    record.dash[3] = second.mix.clamp(0.0, 1.0);
                }
                // PG Step 2/4：纹理来源写进记录。
                // - `View(key)`：`render.rs` 由 `texture != 0` 推出 TEXTURED，按 key 采样 canvas；
                // - `Image`：图像上传进同一张 `image_groups` 表，键空间与 brush rows 分开。
                match &mesh.texture {
                    None => {}
                    Some(TextureSource::View(view_key)) => record.texture = *view_key,
                    Some(TextureSource::Image {
                        width,
                        height,
                        rgba,
                    }) => {
                        let image_key = IMAGE_KEY_BASE + key;
                        if *width == 0 || *height == 0 {
                            return Err(format!(
                                "record {i}: 纹理尺寸不能为 0（{width}×{height}）"
                            ));
                        }
                        let needed = *width as usize * *height as usize * 4;
                        if rgba.len() != needed {
                            return Err(format!(
                                "record {i}: 纹理字节数 {} ≠ {width}×{height}×4 = {needed}",
                                rgba.len()
                            ));
                        }
                        player.add_texture(image_key, *width, *height, rgba)?;
                        record.texture = image_key;
                    }
                }
                built.push(record);
            }
            PathRecord::Points(points) => {
                points
                    .validate()
                    .map_err(|error| format!("record {i}: {error}"))?;
                // 后端的上传格式是 `[f32; 4]`（第 4 个分量当前不被读）。
                let vertices: Vec<[f32; 4]> = points
                    .points
                    .iter()
                    .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, 0.0])
                    .collect();
                player.upload(Upload::points(key, bytemuck::cast_slice(&vertices))?)?;
                let mut record = Record::points(
                    key,
                    vertices.len() as u32,
                    points.model,
                    points.fill,
                    points.radius,
                );
                if let Some(colors) = &points.colors {
                    let row_key = ROW_KEY_BASE | key;
                    player.add_rows(row_key, bytemuck::cast_slice(colors))?;
                    record.fill_rows = row_key;
                }
                built.push(record);
            }
        }
    }
    let record_bytes: Vec<u8> = bytemuck::cast_slice(&built).to_vec();
    Ok(Prepared {
        view: view_bytes,
        records: record_bytes,
        fills,
        programs,
    })
}

/// 相机视图的键段：第 i 个视图用 `(i + 1) << CAMERA_KEY_SHIFT` 起的一段，
/// 与主帧的 1..=n、rows（`1<<40`）、图像纹理（`1<<41`）都不重叠。
const CAMERA_KEY_SHIFT: u64 = 48;

/// 把相机视图逐个打包成后端要的 `(key, width, height, view, records)`。
fn prepare_cameras(
    player: &mut crate::render::Player,
    cameras: &[CameraView],
) -> Result<Vec<crate::CameraFrame>, String> {
    let mut out: Vec<crate::CameraFrame> = Vec::with_capacity(cameras.len());
    for (index, camera) in cameras.iter().enumerate() {
        if camera.key == 0 {
            return Err(format!("相机视图 #{index} 的 key 不能是 0（0 是主帧）"));
        }
        if out.iter().any(|(key, ..)| *key == camera.key) {
            return Err(format!("相机视图 #{index} 的 key {} 重复", camera.key));
        }
        let key_base = (index as u64 + 1) << CAMERA_KEY_SHIFT;
        let prepared = prepare_view(
            player,
            camera.view.clone(),
            camera.width,
            camera.height,
            &camera.records,
            key_base,
        )
        .map_err(|error| format!("相机视图 #{index}（key {}）：{error}", camera.key))?;
        // 着色 codegen 只接主帧：相机视图里的程序会被静默忽略 ⇒ 显式拒绝。
        if !prepared.fills.is_empty() {
            return Err(format!(
                "相机视图 #{index}（key {}）含着色 codegen 填充：零回读路径只支持主帧",
                camera.key
            ));
        }
        out.push((
            camera.key,
            camera.width,
            camera.height,
            prepared.view,
            prepared.records,
        ));
    }
    Ok(out)
}

/// 用给定 view 渲染一帧并读回（native）：同步 API 不变（`frame_loop` / `zela-verify` /
/// `export` 都是同步调用方）。
#[cfg(not(target_arch = "wasm32"))]
fn render_with_view(
    view: Vec<f32>,
    width: u32,
    height: u32,
    samples: u32,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    crate::render::with_gpu(|gpu| {
        let mut player = gpu.take_player(width, height, samples);
        let result = (|| {
            let prepared = prepare_view(&mut player, view, width, height, records, 0)?;
            let cameras = prepare_cameras(&mut player, cameras)?;
            let frames = player.views(&prepared.view, &prepared.records, cameras, prepared.fills, prepared.programs)?;
            let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
            player.pixels(gpu, &frames, &mut out)?;
            Ok(out)
        })();
        gpu.put_player(width, height, samples, player);
        result
    })?
}

/// **B15（甲）**：浏览器上的同一帧——回读用 `await`（不再同步三连）。
#[cfg(target_arch = "wasm32")]
async fn render_with_view_async(
    view: Vec<f32>,
    width: u32,
    height: u32,
    samples: u32,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    let mut slot = crate::render::GpuSlot::take()?;
    let gpu = slot.get();
    let mut player = gpu.take_player(width, height, samples);
    let result = frame_pixels_async(&mut player, gpu, view, width, height, cameras, records).await;
    gpu.put_player(width, height, samples, player);
    result
}

/// 异步读回一帧（调用方负责 player 的取 / 还；错误也归还，缓存不因一次失败丢失）。
#[cfg(target_arch = "wasm32")]
async fn frame_pixels_async(
    player: &mut crate::render::Player,
    gpu: &mut crate::render::Gpu,
    view: Vec<f32>,
    width: u32,
    height: u32,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    let prepared = prepare_view(player, view, width, height, records, 0)?;
    let cameras = prepare_cameras(player, cameras)?;
    let frames = player.views(&prepared.view, &prepared.records, cameras, prepared.fills, prepared.programs)?;
    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    player.pixels_async(gpu, &frames, &mut out).await?;
    Ok(out)
}

/// **B15（乙）**：合成目标必须与帧同尺寸、`Rgba8Unorm`，并带渲染 / 存储 / 采样三种用法。
///
/// 为什么不接受 surface 格式：合成是 compute pass 写 **storage texture**，而 WebGPU 的
/// surface 通常是不支持 `STORAGE_BINDING` 的 `Bgra8Unorm` ⇒ 必须"渲到 Rgba8Unorm，再
/// GPU 内 blit 到 surface"。这正是"零 CPU 往返 ≠ 零拷贝"的口径。
fn check_target(target: &wgpu::Texture, width: u32, height: u32) -> Result<(), String> {
    let size = target.size();
    if size.width != width || size.height != height || size.depth_or_array_layers != 1 {
        return Err(format!(
            "target is {}x{}x{}, not {width}x{height}x1",
            size.width, size.height, size.depth_or_array_layers
        ));
    }
    if target.format() != crate::render::COLOR {
        return Err(format!(
            "target format {:?} cannot take the vector composite (needs {:?} as a storage texture)",
            target.format(),
            crate::render::COLOR
        ));
    }
    let need = wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::STORAGE_BINDING
        | wgpu::TextureUsages::TEXTURE_BINDING;
    let missing = need - target.usage();
    if !missing.is_empty() {
        return Err(format!("target is missing usages {missing:?}"));
    }
    Ok(())
}

/// **B15（乙）**：把一帧直接合成进 `target`（native 同步版），不经过 CPU。
#[cfg(not(target_arch = "wasm32"))]
fn render_to_texture(
    view: Vec<f32>,
    target: &wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<(), String> {
    check_target(target, width, height)?;
    let target_view = target.create_view(&Default::default());
    crate::render::with_gpu(|gpu| {
        let mut player = gpu.take_player(width, height, samples);
        let result = (|| {
            let prepared = prepare_view(&mut player, view, width, height, records, 0)?;
            let cameras = prepare_cameras(&mut player, cameras)?;
            let frames = player.views(&prepared.view, &prepared.records, cameras, prepared.fills, prepared.programs)?;
            player.render_to(gpu, &frames, &target_view)
        })();
        gpu.put_player(width, height, samples, player);
        result
    })?
}

/// **B15（乙）**：同 [`render_to_texture`]，浏览器上的异步版。
#[cfg(target_arch = "wasm32")]
async fn render_to_texture_async(
    view: Vec<f32>,
    target: &wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<(), String> {
    check_target(target, width, height)?;
    let target_view = target.create_view(&Default::default());
    let mut slot = crate::render::GpuSlot::take()?;
    let gpu = slot.get();
    let mut player = gpu.take_player(width, height, samples);
    let result = frame_to_texture_async(
        &mut player,
        gpu,
        view,
        &target_view,
        width,
        height,
        cameras,
        records,
    )
    .await;
    gpu.put_player(width, height, samples, player);
    result
}

/// 异步把一帧合成进 `target`（调用方负责 player 的取 / 还）。
#[cfg(target_arch = "wasm32")]
async fn frame_to_texture_async(
    player: &mut crate::render::Player,
    gpu: &mut crate::render::Gpu,
    view: Vec<f32>,
    target: &wgpu::TextureView,
    width: u32,
    height: u32,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<(), String> {
    let prepared = prepare_view(player, view, width, height, records, 0)?;
    let cameras = prepare_cameras(player, cameras)?;
    let frames = player.views(&prepared.view, &prepared.records, cameras, prepared.fills, prepared.programs)?;
    player.render_to_async(gpu, &frames, target).await
}

/// **阶段 1**：把 [`ParamGeometry`] 接进记录 —— 三种 kind 在后端是两条既有通道：
///
/// - `Lerp`：上传**差值形状**并接到第二项（`key2` / `m2 = mix·I`）。第二项的键区与首形状
///   分开（首形状 `1..=n`，这里从 `n+1` 起），否则同一帧里两条记录会在同一个 `key` 上互相
///   覆盖；相机视图的 `key_base` 是 `(i+1) << 48`，同段内加 `2n` 也不会串到相邻视图。
///   `mix = 0` ⇒ **不挂第二项**（`key2 = 0`）：后端完全跳过它，输出逐位等于普通路径。
/// - `Window` / `Grow`：写 `params[0..2]`（曲线索引 + t）。`Grow` 是 `[0, curves]` 的特例；
///   `curves = 0` 合法（整条不可见，与窗口 `[0,0]` 同口径）。
///
/// 两条通道都不改结构 ⇒ 逐帧只变数字。窗口与 `PathOptions::curve_window` 同义，同时给出
/// 会**显式报错**（不猜哪一个优先）。
fn attach_param(
    player: &mut crate::render::Player,
    record: crate::render::Record,
    param: Option<&ParamGeometry>,
    options: &PathOptions,
    points: &[[f64; 3]],
    ranges: &[[u32; 4]],
    index: usize,
    records: usize,
    key_base: u64,
) -> Result<crate::render::Record, String> {
    let Some(param) = param else { return Ok(record) };
    match param.kind {
        ParamKind::Lerp => {
            let delta = &param.delta;
            let mix = param.mix().expect("Lerp 的 params 恒为 [mix]");
            if delta.len() != points.len() || delta.len() % 4 != 0 {
                return Err(format!(
                    "record {index}: 形变差值控制点数 {} ≠ 首形状 {}（后端逐曲线相加；每段必须 4 个控制点）",
                    delta.len(),
                    points.len()
                ));
            }
            if !mix.is_finite() {
                return Err(format!("record {index}: 形变权重必须是有限数（收到 {mix}）"));
            }
            if mix <= 0.0 {
                return Ok(record);
            }
            let key2 = key_base + records as u64 + index as u64 + 1;
            upload_path(player, key2, index, delta, ranges)?;
            Ok(record.morph(key2, mix.min(1.0)))
        }
        ParamKind::Window | ParamKind::Grow => {
            if options.curve_window.is_some() {
                return Err(format!(
                    "record {index}: 参数化窗口与 `curve_window` 同时给出（二者同义，只留一个）"
                ));
            }
            let [lo, hi] = param
                .window_range()
                .expect("Window / Grow 的 params 形状固定");
            if !(lo.is_finite() && hi.is_finite()) {
                return Err(format!(
                    "record {index}: 窗口端点必须是有限数（收到 [{lo}, {hi}]）"
                ));
            }
            let valid = match param.kind {
                ParamKind::Window => hi > lo,
                ParamKind::Grow => hi >= lo && lo >= 0.0,
                ParamKind::Lerp => unreachable!(),
            };
            if !valid {
                return Err(format!(
                    "record {index}: {:?} 的参数非法（收到 [{lo}, {hi}]；Window 要求 hi > lo，Grow 要求 0 ≤ lo ≤ hi）",
                    param.kind
                ));
            }
            let mut record = record;
            record.params[0] = lo;
            record.params[1] = hi;
            Ok(record)
        }
    }
}

/// 上传一条路径并返回曲线数（填充与描边共用）。
fn upload_path(
    player: &mut crate::render::Player,
    key: u64,
    index: usize,
    points: &[[f64; 3]],
    ranges: &[[u32; 4]],
) -> Result<u32, String> {
    if points.len() % 4 != 0 || points.is_empty() {
        return Err(format!("record {index}: control points are not whole curves"));
    }
    player.upload(Upload::path(
        key,
        bytemuck::cast_slice(points),
        bytemuck::cast_slice(ranges),
        centroid_area(points),
    )?)?;
    Ok((points.len() / 4) as u32)
}

/// **3D 路径 / 网格渲染**（B6 Step 1.2 / B10 Step 2）：`View3`（含灯 / tone / bloom / AO）
/// + 混合记录（路径与网格按声明顺序绘制）。
///
/// **B15**：同步读回的 API 只在 native 存在；浏览器用 [`render_3d_with_async`]
/// （没有阻塞读回，同步签名会踩 `getMappedRange` 的 `OperationError`）。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_3d(view: &View3, records: &[PathRecord]) -> Result<Vec<u8>, String> {
    render_3d_with(view.size.0, view.size.1, 1, view, &[], records)
}

/// 同 [`render_3d`]，但显式给画布尺寸与**采样数**（`frame_loop` 的 3D 分支用 4，
/// 与 2D 路径同口径；`render_3d` 固定 1 是 B6 遗留的调用约定）。
///
/// `cameras`：本帧**先渲**的相机视图（[`CameraView`]）—— 主帧记录按 `key` 采样它们。
/// 空切片 ⇒ 与 PG 之前逐位相同。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_3d_with(
    width: u32,
    height: u32,
    samples: u32,
    view: &View3,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    render_with_view(view_3d(view), width, height, samples, cameras, records)
}

/// 一条绘制记录（保持声明顺序 = 绘制顺序）。
#[derive(Debug, Clone)]
pub enum PathRecord {
    Fill(FilledPath),
    Stroke(StrokedPath),
    /// **B10 Step 2**：3D 网格（可受光）。
    Mesh(MeshRecord),
    /// **PG Step 4**：点云（每个点一个面向相机的圆盘，半径按场景单位）。
    Points(PointsRecord),
}

/// 一团点（PG Step 4）：`points` 是模型空间的 `[x, y, z]`，每个点画一个**同半径**的圆盘。
///
/// 后端本来就有这条通道（`Store::add_points` 的 `Kind::Points`），只是公开面没有记录类型
/// ⇒ `Upload::Points` 一直是死代码。逐点色走与网格同一套 brush rows。
///
/// ⚠️ **半径是每个对象一个**（后端把半径放在实例槽 `extra.x`，`blend.wgsl::disk` 读它），
/// 不是逐点一个 —— 逐点半径需要后端改槽位，本批不做。
#[derive(Debug, Clone, PartialEq)]
pub struct PointsRecord {
    pub points: Vec<[f64; 3]>,
    /// 圆盘半径（场景单位；**直径**写进记录，后端 `extra.x = 半径 × 像素缩放`）。
    pub radius: f32,
    /// 颜色（straight RGBA）；逐点色为 `None` 时用它。
    pub fill: [f32; 4],
    /// 逐点色（长度须等于 `points`；`None` ⇒ 用 `fill`）。
    pub colors: Option<Vec<[f32; 4]>>,
    /// 3×4 模型矩阵（行主序）。
    pub model: [[f32; 4]; 3],
}

impl PointsRecord {
    /// 自洽性检查（显式失败，不静默丢点）。
    pub fn validate(&self) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("points: 点为空".to_string());
        }
        if !(self.radius > 0.0) || !self.radius.is_finite() {
            return Err(format!("points: 半径必须为正（实际 {}）", self.radius));
        }
        if let Some(colors) = &self.colors {
            if colors.len() != self.points.len() {
                return Err(format!(
                    "points: 逐点色 {} ≠ 点数 {}",
                    colors.len(),
                    self.points.len()
                ));
            }
        }
        Ok(())
    }
}

/// **PG Step 2**：一个额外的相机视图 —— 先渲进自己的 canvas，主帧的记录按 `key` 采样它。
///
/// 后端本来就有这条链路（`Player::views` 的 `cameras` + `image_groups` 按 key 采样），
/// 此前公开面四个入口全传 `Vec::new()` ⇒ 场景层没有出口。语义是**任何可渲染内容**：
/// `view` 字节既可以是 2D 正交（[`CameraView::content_2d`]）也可以是 3D 透视
/// （[`CameraView::scene_3d`]），后端不区分。
///
/// 约束（后端既有，且**显式报错**，不静默）：
/// - `key` 非 0，且不得与同一帧的其它视图重复（0 是主帧）；
/// - 只能在**更早**的视图里被采样（`render.rs` 的 `earlier` 判据），即相机视图总在主帧之前渲染；
/// - 主帧记录引用一个不存在的 key ⇒ `unknown texture`。
#[derive(Debug, Clone)]
pub struct CameraView {
    /// 采样键：主帧记录的 `texture` 指它。
    pub key: u64,
    pub width: u32,
    pub height: u32,
    /// 视图字节（由构造器打包，布局见 `view_2d` / `view_3d`）。
    view: Vec<f32>,
    pub records: Vec<PathRecord>,
}

impl CameraView {
    /// 2D 内容视图：世界坐标 = 以画布中心为原点的像素（与 [`render_2d`] 同口径）。
    ///
    /// 这条正是"内容层当纹理源"的形状：图层 draw 已按纹理像素录好，
    /// 渲进 `width×height` 的 canvas 后被 3D 材质按 UV 采样。
    pub fn content_2d(
        key: u64,
        width: u32,
        height: u32,
        background: [f32; 4],
        records: Vec<PathRecord>,
    ) -> Self {
        Self {
            key,
            width,
            height,
            view: view_2d_with(width, height, background, ViewOptions::default()),
            records,
        }
    }

    /// 3D 视图：与 [`render_3d`] 同口径（画中画 / 反射 / 多相机都走这一条）。
    pub fn scene_3d(key: u64, view: &View3, records: Vec<PathRecord>) -> Self {
        Self {
            key,
            width: view.size.0,
            height: view.size.1,
            view: view_3d(view),
            records,
        }
    }
}

impl From<FilledPath> for PathRecord {
    fn from(path: FilledPath) -> Self {
        Self::Fill(path)
    }
}

impl From<StrokedPath> for PathRecord {
    fn from(path: StrokedPath) -> Self {
        Self::Stroke(path)
    }
}

impl From<MeshRecord> for PathRecord {
    fn from(mesh: MeshRecord) -> Self {
        Self::Mesh(mesh)
    }
}

impl From<PointsRecord> for PathRecord {
    fn from(points: PointsRecord) -> Self {
        Self::Points(points)
    }
}

/// **离屏渲染一帧**：`records` 按顺序画到 `width×height` 的 RGBA8 缓冲并读回。
///
/// 返回的字节是行优先 RGBA8（`width * height * 4`）。失败 ⇒ 显式错误（不静默）。
///
/// **B15**：同步版只在 native 存在（浏览器见 [`render_2d_async`]）。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_2d(
    width: u32,
    height: u32,
    samples: u32,
    background: [f32; 4],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    render_2d_with(
        width,
        height,
        samples,
        background,
        ViewOptions::default(),
        records,
    )
}

/// 同 [`render_2d`]，但可带着色栈选项（B6 Step 1.2）。
///
/// `ViewOptions::default()` ⇒ 与 `render_2d` **逐位相同**（既有基线不受影响）。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_2d_with(
    width: u32,
    height: u32,
    samples: u32,
    background: [f32; 4],
    options: ViewOptions,
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err(format!("canvas {width}x{height} is empty"));
    }
    render_with_view(
        view_2d_with(width, height, background, options),
        width,
        height,
        samples,
        &[],
        records,
    )
}

/// **B15（甲）**：浏览器上的 2D 离屏渲染（异步回读；与 [`render_2d`] 逐位同口径）。
#[cfg(target_arch = "wasm32")]
pub async fn render_2d_async(
    width: u32,
    height: u32,
    samples: u32,
    background: [f32; 4],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    render_with_view_async(
        view_2d_with(width, height, background, ViewOptions::default()),
        width,
        height,
        samples,
        &[],
        records,
    )
    .await
}

/// **B15（甲）**：浏览器上的 3D 离屏渲染（异步回读；与 [`render_3d_with`] 逐位同口径）。
#[cfg(target_arch = "wasm32")]
pub async fn render_3d_with_async(
    width: u32,
    height: u32,
    samples: u32,
    view: &View3,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<Vec<u8>, String> {
    render_with_view_async(view_3d(view), width, height, samples, cameras, records).await
}

/// **B15（乙）**：2D 一帧直接合成进 `target`，像素**不经过 CPU**（native 同步版）。
///
/// `target` 必须是 `Rgba8Unorm`、与帧同尺寸，并带
/// `RENDER_ATTACHMENT | STORAGE_BINDING | TEXTURE_BINDING`；不满足 ⇒ 显式报错。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_2d_to_texture(
    target: &wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    background: [f32; 4],
    records: &[PathRecord],
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!("canvas {width}x{height} is empty"));
    }
    render_to_texture(
        view_2d_with(width, height, background, ViewOptions::default()),
        target,
        width,
        height,
        samples,
        &[],
        records,
    )
}

/// **B15（乙）**：3D 一帧直接合成进 `target`（native 同步版）。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_3d_to_texture(
    target: &wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    view: &View3,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<(), String> {
    render_to_texture(view_3d(view), target, width, height, samples, cameras, records)
}

/// **B15（乙）**：2D 一帧直接合成进 `target`（浏览器异步版）。
#[cfg(target_arch = "wasm32")]
pub async fn render_2d_to_texture_async(
    target: &wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    background: [f32; 4],
    records: &[PathRecord],
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!("canvas {width}x{height} is empty"));
    }
    render_to_texture_async(
        view_2d_with(width, height, background, ViewOptions::default()),
        target,
        width,
        height,
        samples,
        &[],
        records,
    )
    .await
}

/// **B15（乙）**：3D 一帧直接合成进 `target`（浏览器异步版）。
#[cfg(target_arch = "wasm32")]
pub async fn render_3d_to_texture_async(
    target: &wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    view: &View3,
    cameras: &[CameraView],
    records: &[PathRecord],
) -> Result<(), String> {
    render_to_texture_async(view_3d(view), target, width, height, samples, cameras, records).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * width + x) * 4) as usize;
        [frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]
    }

    /// **描边宽度口径（B11 §10）**：`StrokedPath::width` 是**宽度**，画出来就该是这么宽。
    ///
    /// `vector.wgsl` 把该槽位读进名为 `half` 的局部量，光读代码很容易得出"要传半宽"
    /// 的错误结论；这条断言是唯一能钉住写入端口径的证据：传 10 就必须量到 ~10 px，
    /// 不是 ~20 px。测量列取线段中部，阈值取半强度（AA 边缘不算入）。
    #[test]
    fn stroke_width_is_the_drawn_width() {
        let (w, h) = (64u32, 64u32);
        let width = 10.0f32;
        // 水平线段 y = 0，从 x = -20 到 x = 20（两条直线段）。
        let line = StrokedPath {
            points: vec![
                [-20.0, 0.0, 0.0],
                [-13.33, 0.0, 0.0],
                [-6.67, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [6.67, 0.0, 0.0],
                [13.33, 0.0, 0.0],
                [20.0, 0.0, 0.0],
            ],
            ranges: vec![[0, 2, 0, 0]],
            stroke: [1.0, 0.0, 0.0, 1.0],
            width,
            shape_bits: 0,
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            options: Default::default(),
            param: None,
        };
        let frame = match render_2d(w, h, 1, [0.0, 0.0, 0.0, 1.0], &[line.clone().into()]) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        let covered = (0..h)
            .filter(|y| pixel(&frame, w, w / 2, *y)[0] > 128)
            .count();
        assert!(
            (covered as f32 - width).abs() <= 1.0,
            "宽度 {width} 应画出 {width} px 的线，实测 {covered} px（半宽/全宽口径混了？）"
        );
    }

    /// 开放子路径（`ranges` 闭合位 = 0）的两条语义，**用像素钉住**（B14 §3.4①）：
    ///
    /// 1. 填充：按**隐式闭合**处理（缺一条边的方形，中央仍被填满）；
    /// 2. 描边：**不回连**（缺的那条边必须是背景）。
    ///
    /// 这两条同时成立，`Polyline` 从"闭合多边形"改成"开放折线"才不会引入新回归。
    #[test]
    fn open_subpath_fills_implicitly_and_strokes_without_closing() {
        let (w, h) = (128u32, 128u32);
        let corners = [
            (-30.0f64, -30.0f64),
            (30.0, -30.0),
            (30.0, 30.0),
            (-30.0, 30.0),
        ];
        let lerp = |a: (f64, f64), b: (f64, f64), t: f64| {
            [a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 0.0]
        };
        let mut points = Vec::new();
        for pair in corners.windows(2) {
            points.push(lerp(pair[0], pair[1], 0.0));
            points.push(lerp(pair[0], pair[1], 1.0 / 3.0));
            points.push(lerp(pair[0], pair[1], 2.0 / 3.0));
            points.push(lerp(pair[0], pair[1], 1.0));
        }
        let ranges = vec![[0, 3, 0, 0]];

        // ① 填充：缺一条边也应被隐式闭合填满。
        let fill = FilledPath {
            points: points.clone(),
            ranges: ranges.clone(),
            fill: [1.0, 0.0, 0.0, 1.0],
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            shade_in_3d: false,
            material: None,
            options: Default::default(),
            param: None,
            shader: None,
        };
        let frame = match render_2d(w, h, 1, [0.0, 0.0, 0.0, 1.0], &[fill.into()]) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        assert!(
            pixel(&frame, w, w / 2, h / 2)[0] > 200,
            "开放子路径的填充应按隐式闭合（中央应为填充色），实测 {:?}",
            pixel(&frame, w, w / 2, h / 2)
        );

        // ② 描边：缺的那条边（世界 x = -30 ⇒ 屏幕 x = 34）不得出现线。
        let stroke = StrokedPath {
            points,
            ranges,
            stroke: [1.0, 0.0, 0.0, 1.0],
            width: 4.0,
            shape_bits: 0,
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            options: Default::default(),
            param: None,
        };
        let frame = match render_2d(w, h, 1, [0.0, 0.0, 0.0, 1.0], &[stroke.into()]) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        assert!(
            pixel(&frame, w, w / 2, 34)[0] > 200,
            "上边（世界 y = -30 ⇒ 屏幕 y = 34）应被描边"
        );
        assert!(
            pixel(&frame, w, 34, h / 2)[0] < 40,
            "缺失的左边缘（屏幕 x = 34）不得被回连描边，实测 {:?}",
            pixel(&frame, w, 34, h / 2)
        );
    }

    /// 矢量路径真的能画出来：中央是填充色，四角是背景色。
    #[test]
    fn filled_square_draws() {
        let (w, h) = (128, 128);
        // 世界坐标 = 以画面中心为原点的像素：±32 px 的正方形。
        let square = FilledPath::polygon(
            &[
                [-32.0, -32.0, 0.0],
                [32.0, -32.0, 0.0],
                [32.0, 32.0, 0.0],
                [-32.0, 32.0, 0.0],
            ],
            [1.0, 0.0, 0.0, 1.0],
        );
        let frame = match render_2d(w, h, 1, [0.0, 0.0, 0.0, 1.0], &[square.clone().into()]) {
            Ok(frame) => frame,
            // 起不来就**响亮失败**：静默 return 会让"什么都没渲染"也算 passed
            // （与 `zela-render-backends/tests/vector_vs_wgpu.rs` 同口径）。
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        let center = pixel(&frame, w, 64, 64);
        assert!(center[0] > 200, "中央应为红色：{center:?}");
        let corner = pixel(&frame, w, 2, 2);
        assert!(corner[0] < 32, "角落应为背景：{corner:?}");
    }

    /// **B6 Step 1.2**：3D view 的槽位布局与上游 `feed.py::view` 一致 ——
    /// 投影矩阵（列主序）、`three_d`、`toward`/bias、eye、灯数、tone、bloom、AO、灯槽。
    #[test]
    fn view3_lays_out_the_same_slots_as_the_upstream_packer() {
        let view = View3 {
            frame: (8.0, 6.0),
            center: [0.0, 0.0, 0.0],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            zoom: 1.0,
            focal: 8.0,
            size: (64, 48),
            background: [0.0, 0.0, 0.0, 1.0],
            light_source: [0.0, -1.0, 0.0],
            exposure: 2.0,
            tone: Some(ze_render_ir::ToneCurve::AgX),
            bloom: 0.5,
            bloom_threshold: 1.0,
            bloom_radius: 4.0,
            ao: Some((0.7, 0.3)),
            lights: vec![
                Light::Sun {
                    direction: [0.0, 0.0, -1.0],
                    color: [1.0, 1.0, 1.0],
                    intensity: 2.0,
                    shadows: true,
                },
                Light::Ambient {
                    color: [1.0, 1.0, 1.0],
                    intensity: 0.1,
                },
            ],
        };
        let v = super::view_3d(&view);
        assert_eq!(v.len(), crate::render::VIEW_LENGTH);
        assert_eq!([v[32], v[33], v[34], v[35]], [64.0, 48.0, 8.0, 1.0]);
        assert_eq!(v[39], 1.0, "three_d");
        assert_eq!([v[40], v[41], v[42]], [0.0, 0.0, 1.0], "toward = 相机 z 轴");
        assert_eq!(v[43], super::ON_3D, "3D 路径的深度偏置");
        assert_eq!([v[48], v[49], v[50], v[51]], [0.0, 0.0, 8.0, 1.0], "eye = center + f·z");
        assert_eq!(v[super::VIEW_LIGHTING + 4], 2.0, "灯数");
        assert_eq!(v[super::VIEW_LIGHTING + 5], 2.0, "曝光");
        assert_eq!(v[super::VIEW_LIGHTING + 6], 1.0, "tone = AgX");
        assert_eq!(v[super::VIEW_LIGHTING + 7], 0.5, "bloom 强度");
        assert_eq!(v[super::VIEW_OCCLUSION], 0.7, "AO 强度");
        assert_eq!(v[super::VIEW_OCCLUSION + 1], 0.3, "AO 半径");
        // 第一盏灯：方向 + kind=1 + 线性色 × 强度 + 投影位。
        let sun = super::VIEW_LIGHTING + 8;
        assert_eq!([v[sun], v[sun + 1], v[sun + 2]], [0.0, 0.0, -1.0]);
        assert_eq!(v[sun + 3], 1.0);
        assert!((v[sun + 4] - 2.0).abs() < 1e-6, "线性白 × 2");
        assert_eq!(v[sun + 13], 1.0, "casts shadows");
        // 第二盏：kind=0（环境光）。
        let ambient = sun + 16;
        assert_eq!(v[ambient + 3], 0.0);
        assert!((v[ambient + 4] - 0.1).abs() < 1e-3);
    }

    /// **B6 Step 1.2 验收**：着色栈真的接上了 —— 同一帧在
    /// ① 默认（clip）② `exposure = 4 + AgX` 下必须**不同**；
    /// 且默认选项与 `render_2d` 逐位相同（默认等效）。
    #[test]
    fn tone_mapping_changes_pixels_and_defaults_are_identical() {
        let (w, h) = (64, 64);
        let square = FilledPath::polygon(
            &[
                [-16.0, -16.0, 0.0],
                [16.0, -16.0, 0.0],
                [16.0, 16.0, 0.0],
                [-16.0, 16.0, 0.0],
            ],
            [0.5, 0.5, 0.5, 1.0],
        );
        let records = [PathRecord::from(square)];
        let plain = match render_2d(w, h, 1, [0.0, 0.0, 0.0, 1.0], &records) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        let same = match render_2d_with(
            w,
            h,
            1,
            [0.0, 0.0, 0.0, 1.0],
            ViewOptions::default(),
            &records,
        ) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        assert_eq!(plain, same, "默认选项必须逐位不变");

        let toned = match render_2d_with(
            w,
            h,
            1,
            [0.0, 0.0, 0.0, 1.0],
            ViewOptions {
                exposure: 4.0,
                // ⚠️ 上游把 tone 放在 **bloom 的合成 pass** 里（`render.rs:2429`），
                // 所以只开 tone 不开 bloom 时画面不变 —— 这不是没接上，是上游的管线结构。
                tone: Some(ze_render_ir::ToneCurve::AgX),
                bloom_strength: 1.0,
                ..Default::default()
            },
            &records,
        ) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        let clipped = match render_2d_with(
            w,
            h,
            1,
            [0.0, 0.0, 0.0, 1.0],
            ViewOptions {
                exposure: 4.0,
                tone: Some(ze_render_ir::ToneCurve::Clip),
                bloom_strength: 1.0,
                ..Default::default()
            },
            &records,
        ) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        // ⚠️ 上游把 tone 放在 **bloom 的合成 pass** 里，而那条 pass 只在
        // **存在 3D layered 计划**时才跑（`render.rs:2419` 的 `let Some(plan) = plan else { return }`）。
        // ⇒ 纯 2D 帧上 tone / bloom 槽位**没有消费者**（不是我们没填）。
        // 本用例把这个结构事实钉住：槽位填了、但 2D-only 帧不变 ⇒ 将来接 3D 视图时
        // 必须补一条"有 3D 时 AgX ≠ clip"的用例（GAPS V2-15）。
        assert_eq!(
            pixel(&toned, w, 32, 32),
            pixel(&clipped, w, 32, 32),
            "2D-only 帧上 tone 应无消费者（上游结构）；若这里开始不同，说明 2D 路径结构变了，请更新 V2-15"
        );
    }

    // ─── B6 Step 1 验收：3D 视图上的着色栈（tone / bloom / AO）真的有像素效果 ────

    /// 固定场景：一盏强方向光 + 环境光 + 一块 ±1 世界单位的**受光**平面路径。
    ///
    /// 为什么必须带 `material`：`plan.lit` 由 `vector.rs:1058` 按 `material[3] > 0` 判定，
    /// 而 bloom / tone 的合成 pass 的入场券 `glow` 要求 `light || plan.lit`
    /// （`render.rs:2182`）—— 没有材质就没有那条 pass，tone 与 bloom 都不会生效。
    fn lit_square() -> FilledPath {
        let mut path = FilledPath::polygon(
            &[
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
            [0.8, 0.8, 0.8, 1.0],
        );
        path.shade_in_3d = true;
        path.material = Some([0.0, 0.15, 1.0]); // 低粗糙、全反射
        path
    }

    fn lit_view(
        tone: Option<ze_render_ir::ToneCurve>,
        bloom: f32,
        ao: Option<(f32, f32)>,
        ambient: bool,
    ) -> View3 {
        let mut lights = vec![Light::Sun {
            direction: [0.0, 0.0, -1.0],
            color: [1.0, 1.0, 1.0],
            intensity: 4.0,
            shadows: false,
        }];
        if ambient {
            lights.push(Light::Ambient {
                color: [1.0, 1.0, 1.0],
                intensity: 1.0,
            });
        }
        View3 {
            frame: (4.0, 4.0),
            center: [0.0, 0.0, 0.0],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            zoom: 1.0,
            focal: 8.0,
            size: (64, 64),
            background: [0.0, 0.0, 0.0, 1.0],
            light_source: [0.0, -1.0, 0.0],
            exposure: 2.0,
            tone,
            bloom,
            bloom_threshold: 0.0,
            bloom_radius: 4.0,
            ao,
            lights,
        }
    }

    fn lit_frame(
        tone: Option<ze_render_ir::ToneCurve>,
        bloom: f32,
        ao: Option<(f32, f32)>,
        ambient: bool,
    ) -> Vec<u8> {
        let records = [PathRecord::from(lit_square())];
        match render_3d(&lit_view(tone, bloom, ao, ambient), &records) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量 3D 后端起不来：{error}"),
        }
    }

    /// **B6 Step 1 验收（tone）**：有 3D 计划时 `AgX ≠ clip`，且 clip 下高光死白、
    /// AgX 下被压回可分辨区间 —— 这正是 V2-15 记的"2D-only 帧无消费者"的补证。
    #[test]
    fn tone_mapping_changes_a_lit_3d_frame() {
        let clip = lit_frame(Some(ze_render_ir::ToneCurve::Clip), 1.0, None, true);
        let agx = lit_frame(Some(ze_render_ir::ToneCurve::AgX), 1.0, None, true);
        let at = pixel(&clip, 64, 32, 32);
        let bt = pixel(&agx, 64, 32, 32);
        assert_ne!(at, bt, "有 3D 计划时 AgX 必须与 clip 不同：{at:?} vs {bt:?}");
        assert!(
            at[..3].iter().all(|c| *c >= 250),
            "clip 下高光应死白：{at:?}"
        );
        assert!(
            bt[..3].iter().any(|c| *c < 250),
            "AgX 下高光应被压回：{bt:?}"
        );
    }

    /// **B6 Step 1 验收（bloom）**：亮区外溢 —— 方块外 3 像素处的亮度必须上升。
    ///
    /// 方块占 ±1 世界单位、画幅 4 ⇒ 每单位 16 px，边缘在 y = 16（上）/ 48（下）；
    /// 采样点 `(32, 13)` 在方块外 3 px。
    #[test]
    fn bloom_spreads_bright_pixels_outward() {
        let off = lit_frame(Some(ze_render_ir::ToneCurve::AgX), 0.0, None, true);
        let on = lit_frame(Some(ze_render_ir::ToneCurve::AgX), 1.0, None, true);
        let luma = |p: [u8; 4]| p[0] as i32 + p[1] as i32 + p[2] as i32;
        let outside_off = luma(pixel(&off, 64, 32, 13));
        let outside_on = luma(pixel(&on, 64, 32, 13));
        assert!(
            outside_on > outside_off,
            "bloom 应让亮区外溢：off = {outside_off}，on = {outside_on}"
        );
    }

    /// **PB Step 2（V-02）验收**：`threshold` 真的切掉了低亮度扩散。
    ///
    /// 同一帧、同一个 bloom 强度：门槛 0（旧行为）在方块外 3 px 有光晕；门槛 10（远高于
    /// 场景亮度）什么都不扩散 ⇒ 与"关掉 bloom"同亮。这是契约第二个字段的消费者。
    #[test]
    fn bloom_threshold_stops_low_light_from_spreading() {
        let spread = |threshold: f32| {
            let mut view = lit_view(Some(ze_render_ir::ToneCurve::AgX), 1.0, None, true);
            view.bloom_threshold = threshold;
            let records = [PathRecord::from(lit_square())];
            render_3d(&view, &records).expect("3D 渲染失败")
        };
        let luma = |p: [u8; 4]| p[0] as i32 + p[1] as i32 + p[2] as i32;
        let open = spread(0.0);
        let cut = spread(10.0);
        let off = lit_frame(Some(ze_render_ir::ToneCurve::AgX), 0.0, None, true);
        let (a, b, c) = (
            luma(pixel(&open, 64, 32, 13)),
            luma(pixel(&cut, 64, 32, 13)),
            luma(pixel(&off, 64, 32, 13)),
        );
        assert!(b < a, "门槛必须切掉扩散：open = {a}，cut = {b}");
        assert!(
            (b - c).abs() <= 3,
            "门槛 10 ⇒ 与关掉 bloom 同亮（bloom off = {c}，cut = {b}）"
        );
    }

    /// **B10 Step 2 验收（网格）**：三角形网格在 3D 视图里真的画出来，且
    /// `material = Some(_)` 让它走"逐片元受光"，于是 tone 映射（bloom 合成 pass 里的
    /// AgX）真的改像素 —— 这正是 §0.2 那条链（`Kind::Mesh` → material → plan.lit →
    /// glow → bloom/tone）的入口。
    #[test]
    fn mesh_records_draw_and_are_lit() {
        let mesh = |material: Option<[f32; 3]>| MeshRecord {
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
            uvs: None,
            normals: None,
            triangles: vec![0, 1, 2, 0, 2, 3],
            fill: [0.8, 0.2, 0.2, 1.0],
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            material,
            colors: None,
            second: None,
            texture: None,
        };
        let view = |tone: ze_render_ir::ToneCurve| View3 {
            frame: (4.0, 4.0),
            center: [0.0, 0.0, 0.0],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            zoom: 1.0,
            focal: 8.0,
            size: (64, 64),
            background: [0.0, 0.0, 0.0, 1.0],
            light_source: [0.0, 0.0, 1.0],
            exposure: 4.0,
            tone: Some(tone),
            bloom: 1.0,
            bloom_threshold: 0.0,
            bloom_radius: 0.0,
            ao: None,
            lights: vec![
                // 光来自 +Z（相机在 +Z 侧，看到的是被照亮的一面）。
                Light::Sun {
                    direction: [0.0, 0.0, 1.0],
                    color: [1.0, 1.0, 1.0],
                    intensity: 3.0,
                    shadows: false,
                },
                Light::Ambient {
                    color: [1.0, 1.0, 1.0],
                    intensity: 0.4,
                },
            ],
        };
        let frame = |material: Option<[f32; 3]>, tone: ze_render_ir::ToneCurve| {
            let records = [PathRecord::from(mesh(material))];
            match render_3d(&view(tone), &records) {
                Ok(frame) => frame,
                Err(error) => panic!("矢量 3D 网格起不来：{error}"),
            }
        };

        let lit = frame(Some([0.0, 0.5, 1.0]), ze_render_ir::ToneCurve::AgX);
        let center = pixel(&lit, 64, 32, 32);
        assert!(
            center[..3].iter().any(|c| *c > 16),
            "网格应真的画出来（中央非背景）：{center:?}"
        );

        let clip = frame(Some([0.0, 0.5, 1.0]), ze_render_ir::ToneCurve::Clip);
        assert_ne!(
            pixel(&clip, 64, 32, 32),
            center,
            "有材质的网格 ⇒ plan.lit ⇒ tone（bloom 合成 pass）必须生效：AgX ≠ clip"
        );

        let unlit = frame(None, ze_render_ir::ToneCurve::AgX);
        assert_ne!(
            pixel(&unlit, 64, 32, 32),
            center,
            "material = None 时不应走受光路径（与 Some 必须不同）"
        );
    }

    /// 网格几何自洽性：空顶点 / 非三元组索引 / 越界索引 / UV 数不匹配都显式报错。
    #[test]
    fn mesh_record_validates_its_geometry() {
        let base = MeshRecord {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uvs: None,
            normals: None,
            triangles: vec![0, 1, 2],
            fill: [1.0; 4],
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            material: None,
            colors: None,
            second: None,
            texture: None,
        };
        base.validate().expect("合法三角面");

        let mut empty = base.clone();
        empty.positions.clear();
        assert!(empty.validate().is_err());

        let mut partial = base.clone();
        partial.triangles = vec![0, 1];
        assert!(partial.validate().is_err());

        let mut out_of_range = base.clone();
        out_of_range.triangles = vec![0, 1, 9];
        assert!(out_of_range.validate().is_err());

        let mut bad_uv = base.clone();
        bad_uv.uvs = Some(vec![[0.0, 0.0]; 2]);
        assert!(bad_uv.validate().is_err());
    }

    /// **PG Step 2 判别实验 G-1**：相机视图能被主帧的网格按 `texture` 采样到。
    ///
    /// 两块内容：相机视图里一整块**红**；主帧一块网格本来是**绿**、UV 覆盖 0..1。
    /// - `texture = Some(View(key))` ⇒ 采样到红（红通道压过绿）—— 证明"链路真的服务这个场景"；
    /// - `texture = None`          ⇒ 还是绿 —— 证明红**不是**别处漏进来的；
    /// - `View(不存在的 key)`       ⇒ 显式 `unknown texture`（不静默画白）。
    #[test]
    fn a_camera_view_is_sampled_by_a_mesh() {
        let red = FilledPath::polygon(
            &[
                [-40.0, -40.0, 0.0],
                [40.0, -40.0, 0.0],
                [40.0, 40.0, 0.0],
                [-40.0, 40.0, 0.0],
            ],
            [1.0, 0.0, 0.0, 1.0],
        );
        // 相机视图铺满画布（2D 视图 1 世界单位 = 1 px ⇒ ±40 覆盖 64×64 还有余）。
        let camera = CameraView::content_2d(
            7,
            64,
            64,
            [0.0, 0.0, 0.0, 0.0],
            vec![PathRecord::Fill(red)],
        );
        // 主帧：一块 ±1 世界单位的四边形（画幅 4 ⇒ 占中间 32×32），UV 覆盖整张纹理，
        // `v = 1` 在 +y（图像上沿）。
        let quad = |texture: Option<TextureSource>| {
            PathRecord::Mesh(MeshRecord {
                positions: vec![
                    [-1.0, -1.0, 0.0],
                    [1.0, -1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [-1.0, 1.0, 0.0],
                ],
                uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
                normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
                triangles: vec![0, 1, 2, 0, 2, 3],
                fill: [0.0, 1.0, 0.0, 1.0],
                model: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
                material: None,
                colors: None,
                second: None,
                texture,
            })
        };
        let view = lit_view(None, 0.0, None, true);
        let frame = |texture: Option<TextureSource>| {
            render_3d_with(64, 64, 1, &view, &[camera.clone()], &[quad(texture)])
                .unwrap_or_else(|error| panic!("相机视图渲染失败：{error}"))
        };

        let sampled = pixel(&frame(Some(TextureSource::View(7))), 64, 32, 32);
        assert!(
            sampled[0] > 180 && sampled[1] < 80,
            "网格应采样到相机视图的红：{sampled:?}"
        );
        let plain = pixel(&frame(None), 64, 32, 32);
        assert!(
            plain[1] > 180 && plain[0] < 80,
            "不带纹理时应还是网格自身的绿：{plain:?}"
        );

        let error = render_3d_with(
            64,
            64,
            1,
            &view,
            &[camera],
            &[quad(Some(TextureSource::View(9)))],
        )
        .expect_err("采样不存在的 key 必须显式报错");
        assert!(error.contains("unknown texture"), "{error}");
    }

    /// **PG Step 4（纹理资产）**：网格可以自带一张 RGBA8 图像（与相机视图共用采样出口）。
    ///
    /// 判据 = "改变输出"：同一块绿网格，贴上 1×1 红图 ⇒ 红；不给纹理 ⇒ 绿。
    #[test]
    fn an_inline_texture_changes_the_mesh() {
        let quad = |texture: Option<TextureSource>| {
            PathRecord::Mesh(MeshRecord {
                positions: vec![
                    [-1.0, -1.0, 0.0],
                    [1.0, -1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [-1.0, 1.0, 0.0],
                ],
                uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
                normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
                triangles: vec![0, 1, 2, 0, 2, 3],
                fill: [0.0, 1.0, 0.0, 1.0],
                model: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
                material: None,
                colors: None,
                second: None,
                texture,
            })
        };
        let view = lit_view(None, 0.0, None, true);
        let red_image = TextureSource::Image {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        };
        let textured = render_3d_with(64, 64, 1, &view, &[], &[quad(Some(red_image.clone()))])
            .expect("内联纹理应能渲染");
        let center = pixel(&textured, 64, 32, 32);
        assert!(
            center[0] > 180 && center[1] < 80,
            "贴上红图后网格应是红的：{center:?}"
        );
        let plain = render_3d_with(64, 64, 1, &view, &[], &[quad(None)]).expect("无纹理");
        let center = pixel(&plain, 64, 32, 32);
        assert!(
            center[1] > 180 && center[0] < 80,
            "无纹理时应是网格自身的绿：{center:?}"
        );

        let error = render_3d_with(
            64,
            64,
            1,
            &view,
            &[],
            &[quad(Some(TextureSource::Image {
                width: 2,
                height: 2,
                rgba: vec![0; 4],
            }))],
        )
        .expect_err("纹理字节数不符必须显式报错");
        assert!(error.contains("纹理字节数"), "{error}");
    }

    /// **PG Step 4（点云逐点色）**：每个点一个圆盘，逐点色取代单一 `fill`。
    #[test]
    fn per_point_colors_change_a_point_cloud() {
        let points = |colors: Option<Vec<[f32; 4]>>| PointsRecord {
            points: vec![[-10.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            radius: 6.0,
            fill: [1.0, 1.0, 1.0, 1.0],
            colors,
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        };
        let clear = [0.0, 0.0, 0.0, 1.0];
        let plain = render_2d_or_panic(&[PathRecord::Points(points(None))], clear);
        let left = pixel(&plain, 64, 22, 32);
        assert!(left[0] > 180 && left[1] > 180, "无逐点色时用 fill（白）：{left:?}");

        let colored = render_2d_or_panic(
            &[PathRecord::Points(points(Some(vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ])))],
            clear,
        );
        let (a, b) = (pixel(&colored, 64, 22, 32), pixel(&colored, 64, 42, 32));
        assert!(a[0] > 160 && a[2] < 80, "左点应是逐点色的红：{a:?}");
        assert!(b[2] > 160 && b[0] < 80, "右点应是逐点色的蓝：{b:?}");

        let error = render_2d(
            64,
            64,
            1,
            clear,
            &[PathRecord::Points(PointsRecord {
                colors: Some(vec![[1.0; 4]]),
                ..points(None)
            })],
        )
        .expect_err("逐点色数不符必须显式报错");
        assert!(error.contains("逐点色"), "{error}");
    }

    /// **闭合孔洞**：同一路径里的两条子路径（外圈 CCW + 内圈 CW）按 nonzero 规则
    /// 必须留下孔；孔被填满说明填充规则 / 子路径方向在某一层丢了。
    ///
    /// 这是 `vector_paths` 里字母 `o` / `e` 计数器（counter）被填色的最小复现。
    #[test]
    fn concave_ring_keeps_its_hole() {
        let ring = |points: &[[f64; 3]]| {
            let n = points.len();
            let mut curves = Vec::new();
            for i in 0..n {
                let a = points[i];
                let b = points[(i + 1) % n];
                let lerp = |t: f64| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
                curves.extend_from_slice(&[a, lerp(1.0 / 3.0), lerp(2.0 / 3.0), b]);
            }
            FilledPath {
                points: curves,
                ranges: vec![[0, n as u32, 1, 0]],
                fill: [1.0, 0.0, 0.0, 1.0],
                model: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
                shade_in_3d: false,
                material: None,
                options: Default::default(),
                param: None,
                shader: None,
            }
        };
        // 外圈 CCW、内圈 CW（世界 y 向上）。
        // 2D 视图里 1 世界单位 = 1 px（`projection = 1/(size/2)`）⇒ 外圈 ±16、内圈 ±8。
        let mut merged = ring(&[
            [-16.0, -16.0, 0.0],
            [16.0, -16.0, 0.0],
            [16.0, 16.0, 0.0],
            [-16.0, 16.0, 0.0],
        ]);
        let inner = ring(&[
            [-8.0, -8.0, 0.0],
            [-8.0, 8.0, 0.0],
            [8.0, 8.0, 0.0],
            [8.0, -8.0, 0.0],
        ]);
        let offset = (merged.points.len() / 4) as u32;
        merged.points.extend_from_slice(&inner.points);
        merged.ranges.extend(
            inner
                .ranges
                .iter()
                .map(|r| [r[0] + offset, r[1] + offset, r[2], r[3]]),
        );

        let frame = match render_2d(64, 64, 1, [0.0, 0.0, 0.0, 1.0], &[merged.into()]) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        };
        let center = pixel(&frame, 64, 32, 32);
        // 环带中点：(32, 20) 在外圈 16 与内圈 24 之间。
        let edge = pixel(&frame, 64, 32, 20);
        assert!(edge[0] > 200, "环带应被填充：{edge:?}");
        assert!(center[0] < 32, "孔洞必须保持背景色：{center:?}");
    }

    /// **B6 Step 1 验收（AO）**：AO 是屏幕空间的深度遮挡，**单块平面没有凹陷 ⇒ 不会变暗**
    /// （这是正确行为，不是没接上）。所以用"墙 + 地板"的内角来验：有环境光时内角变暗；
    /// 只有方向光时 AO **无效**（上游 `occlusion_of()` 的结构）。
    #[test]
    fn ambient_occlusion_requires_ambient_light() {
        // 墙（z = −1）与地板（y = −1）在 y = −1, z = −1 处形成内角。
        let corner = || {
            let mut wall = FilledPath::polygon(
                &[
                    [-1.0, -1.0, -1.0],
                    [1.0, -1.0, -1.0],
                    [1.0, 1.0, -1.0],
                    [-1.0, 1.0, -1.0],
                ],
                [0.8, 0.8, 0.8, 1.0],
            );
            let mut floor = FilledPath::polygon(
                &[
                    [-1.0, -1.0, -1.0],
                    [1.0, -1.0, -1.0],
                    [1.0, -1.0, 0.0],
                    [-1.0, -1.0, 0.0],
                ],
                [0.8, 0.8, 0.8, 1.0],
            );
            for path in [&mut wall, &mut floor] {
                path.shade_in_3d = true;
                path.material = Some([0.0, 0.9, 0.0]); // 漫反射为主
            }
            vec![PathRecord::from(wall), PathRecord::from(floor)]
        };
        let frame = |ao: Option<(f32, f32)>, ambient: bool| {
            let mut view = lit_view(Some(ze_render_ir::ToneCurve::AgX), 0.0, ao, ambient);
            // 只看环境光：AO 乘的正是环境光项（方向光不受 AO 影响）。
            view.lights.retain(|light| {
                if ambient {
                    true
                } else {
                    matches!(light, Light::Sun { .. })
                }
            });
            match render_3d(&view, &corner()) {
                Ok(frame) => frame,
                Err(error) => panic!("矢量 3D 后端起不来：{error}"),
            }
        };
        let luma_sum = |frame: &[u8]| frame.chunks_exact(4).map(|p| p[0] as i64 + p[1] as i64 + p[2] as i64).sum::<i64>();

        let plain = frame(None, true);
        let occluded = frame(Some((1.0, 0.3)), true);
        assert_ne!(plain, occluded, "有环境光时 AO 必须改变像素");
        let (before, after) = (luma_sum(&plain), luma_sum(&occluded));
        assert!(
            after < before,
            "内角应被 AO 压暗：before = {before}，after = {after}"
        );

        let sun_only = frame(None, false);
        let sun_only_occluded = frame(Some((1.0, 0.3)), false);
        assert_eq!(
            sun_only, sun_only_occluded,
            "只有方向光时 AO 不生效（上游 `occlusion_of` 依赖环境光）；若这里开始不同，请更新 V2-15"
        );
    }

    // ─── PD：逐参验证（每个新参数都要证明"它真的改变输出"） ─────────────────

    /// **PF Step 2 / E-3（可证伪）**：`overlay` 记录固定压在**屏幕平面**上。
    ///
    /// 同一条路径放在 `z = 3`：不带 overlay ⇒ 透视把它放大（相机 focal = 8，
    /// 1/(1−3/8) = 1.6 倍），边缘越出屏幕方块的范围；带 overlay ⇒ 与 z = 0 完全同位。
    /// 证伪：若 `apply_options` 没写 OVERLAY 位，两侧像素相同 ⇒ 用例失败。
    #[test]
    fn overlay_path_lands_on_the_screen_plane_in_a_3d_frame() {
        let view = lit_view(None, 0.0, None, true);
        let path = |overlay: bool| {
            let mut path = FilledPath::polygon(
                &[
                    [-1.0, -1.0, 3.0],
                    [1.0, -1.0, 3.0],
                    [1.0, 1.0, 3.0],
                    [-1.0, 1.0, 3.0],
                ],
                [0.0, 1.0, 0.0, 1.0],
            );
            path.options.overlay = overlay;
            PathRecord::Fill(path)
        };
        let overlay = render_3d(&view, &[path(true)]).expect("3D 渲染失败");
        let plain = render_3d(&view, &[path(false)]).expect("3D 渲染失败");
        // 64×64、frame = 4 世界单位 ⇒ 世界 ±1 = 像素 16..48（overlay 矩阵 1/hw，hw = 2）。
        assert_eq!(pixel(&overlay, 64, 32, 32)[1], 255, "overlay 应盖住中心");
        assert!(
            pixel(&overlay, 64, 10, 10)[1] < 40,
            "overlay 的世界 ±1 不得因 z 变大（{:?}）",
            pixel(&overlay, 64, 10, 10)
        );
        assert!(
            pixel(&plain, 64, 10, 10)[1] > 40,
            "不带 overlay 时 z = 3 会被透视放大（{:?}）",
            pixel(&plain, 64, 10, 10)
        );
    }

    fn square8(fill: [f32; 4]) -> FilledPath {
        FilledPath::polygon(
            &[
                [-8.0, -8.0, 0.0],
                [8.0, -8.0, 0.0],
                [8.0, 8.0, 0.0],
                [-8.0, 8.0, 0.0],
            ],
            fill,
        )
    }

    fn render_2d_or_panic(records: &[PathRecord], clear: [f32; 4]) -> Vec<u8> {
        match render_2d(64, 64, 1, clear, records) {
            Ok(frame) => frame,
            Err(error) => panic!("矢量后端起不来：{error}"),
        }
    }

    fn count(frame: &[u8], test: impl Fn(&[u8]) -> bool) -> usize {
        frame.chunks_exact(4).filter(|p| test(p)).count()
    }

    /// **PD（background / params[3]）**：第三色 + 背景描边宽度真的画出外框。
    #[test]
    fn background_stroke_changes_pixels_outside_the_fill() {
        let red = |p: &[u8]| p[0] > 160 && p[1] < 80 && p[2] < 80;
        let clear = [0.0, 0.0, 0.0, 1.0];
        let plain = render_2d_or_panic(&[square8([1.0, 1.0, 1.0, 1.0]).into()], clear);
        let mut framed = square8([1.0, 1.0, 1.0, 1.0]);
        framed.options.background = Some([1.0, 0.0, 0.0, 1.0]);
        framed.options.background_width = 4.0;
        let framed = render_2d_or_panic(&[framed.into()], clear);
        assert_eq!(count(&plain, red), 0, "不给 background 时不该出现红色");
        assert!(
            count(&framed, red) > 50,
            "背景描边应画出红色外框（实际 {} px）",
            count(&framed, red)
        );
    }

    /// **PD（second paint / dash[3]）**：第二套 paint 在 `mix = 1` 时完全取代第一套。
    #[test]
    fn second_paint_replaces_the_fill_at_mix_one() {
        let clear = [0.0, 0.0, 0.0, 1.0];
        let mut first = square8([1.0, 0.0, 0.0, 1.0]);
        first.options.second = Some(SecondPaint {
            fill: [0.0, 0.0, 1.0, 1.0],
            stroke: [0.0, 0.0, 0.0, 0.0],
            background: [0.0; 4],
            mix: 0.0,
        });
        let mut second = first.clone();
        second.options.second.as_mut().expect("second").mix = 1.0;
        let a = pixel(&render_2d_or_panic(&[first.into()], clear), 64, 32, 32);
        let b = pixel(&render_2d_or_panic(&[second.into()], clear), 64, 32, 32);
        assert!(a[0] > 200 && a[2] < 32, "mix 0 应仍是红色：{a:?}");
        assert!(b[2] > 200 && b[0] < 32, "mix 1 应变成蓝色：{b:?}");
    }

    /// **PD（dash）**：虚线让描边覆盖率下降（period/duty 真的参与）。
    #[test]
    fn dash_breaks_a_stroke() {
        let line = |dash: Option<Dash>| StrokedPath {
            points: vec![
                [-16.0, 0.0, 0.0],
                [-10.0, 0.0, 0.0],
                [-6.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [6.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [16.0, 0.0, 0.0],
            ],
            ranges: vec![[0, 2, 0, 0]],
            stroke: [1.0, 1.0, 1.0, 1.0],
            width: 6.0,
            shape_bits: 0,
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            options: PathOptions {
                dash,
                ..Default::default()
            },
            param: None,
        };
        let clear = [0.0, 0.0, 0.0, 1.0];
        let white = |p: &[u8]| p[0] > 160 && p[1] > 160 && p[2] > 160;
        let solid = render_2d_or_panic(&[line(None).into()], clear);
        let dashed = render_2d_or_panic(
            &[line(Some(Dash {
                period: 0.25,
                duty: 0.5,
                phase: 0.0,
            }))
            .into()],
            clear,
        );
        assert!(
            count(&dashed, white) < count(&solid, white),
            "虚线应比实线少：solid = {}，dashed = {}",
            count(&solid, white),
            count(&dashed, white)
        );
        assert!(count(&dashed, white) > 0, "虚线不该整条消失");
    }

    /// **PD（curve_window / params[0..2]）**：窗口只显示前半段。
    #[test]
    fn curve_window_shows_only_the_first_curve() {
        let line = |window: Option<[f32; 2]>| StrokedPath {
            points: vec![
                [-16.0, 0.0, 0.0],
                [-10.0, 0.0, 0.0],
                [-6.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [6.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [16.0, 0.0, 0.0],
            ],
            ranges: vec![[0, 2, 0, 0]],
            stroke: [1.0, 1.0, 1.0, 1.0],
            width: 6.0,
            shape_bits: 0,
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            options: PathOptions {
                curve_window: window,
                ..Default::default()
            },
            param: None,
        };
        let clear = [0.0, 0.0, 0.0, 1.0];
        let white = |p: &[u8]| p[0] > 160 && p[1] > 160 && p[2] > 160;
        let full = render_2d_or_panic(&[line(None).into()], clear);
        let first = render_2d_or_panic(&[line(Some([0.0, 1.0])).into()], clear);
        // 右半段（世界 x = +12 ⇒ 屏幕 x = 44）。
        assert!(white(&pixel(&full, 64, 44, 32)), "整条应画到右端");
        assert!(
            !white(&pixel(&first, 64, 44, 32)),
            "窗口 [0,1] 不该画到第二段"
        );
        assert!(count(&first, white) < count(&full, white));
    }

    /// **PD（gradient）**：多 stop 沿轴变化，且 stop 数真的参与（2 vs 3 的中点不同）。
    #[test]
    fn gradient_stops_vary_along_the_axis() {
        let clear = [0.0, 0.0, 0.0, 1.0];
        let square = |stops: Vec<[f32; 4]>| {
            let mut path = square8([1.0, 0.0, 0.0, 1.0]);
            path.options.gradient = Some(Gradient {
                stops,
                from: [-8.0, 0.0, 0.0],
                to: [8.0, 0.0, 0.0],
            });
            path
        };
        let two = render_2d_or_panic(
            &[square(vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ])
            .into()],
            clear,
        );
        let (left, right) = (pixel(&two, 64, 26, 32), pixel(&two, 64, 38, 32));
        assert!(
            left[0] > right[0] + 40 && right[2] > left[2] + 40,
            "红→蓝渐变应沿轴变化：left = {left:?}，right = {right:?}"
        );
        // 3 stop 的中点（绿）必须与 2 stop 的中点（红蓝之间）不同。
        let three = render_2d_or_panic(
            &[square(vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ])
            .into()],
            clear,
        );
        let (mid_two, mid_three) = (pixel(&two, 64, 32, 32), pixel(&three, 64, 32, 32));
        assert!(
            mid_three[1] > mid_two[1] + 40,
            "多 stop 应改变中点颜色：2 stop = {mid_two:?}，3 stop = {mid_three:?}"
        );
    }

    /// **PD（逐顶点色）**：网格的 brush rows 按顶点取色，取代单一 `fill`。
    #[test]
    fn per_vertex_colors_replace_the_mesh_fill() {
        let mesh = MeshRecord {
            positions: vec![
                [-8.0, -8.0, 0.0],
                [0.0, -8.0, 0.0],
                [0.0, 8.0, 0.0],
                [-8.0, 8.0, 0.0],
                [0.0, -8.0, 0.0],
                [8.0, -8.0, 0.0],
                [8.0, 8.0, 0.0],
                [0.0, 8.0, 0.0],
            ],
            uvs: None,
            normals: None,
            triangles: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            fill: [1.0, 1.0, 1.0, 1.0],
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            material: None,
            colors: Some(vec![
                [1.0, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ]),
            second: None,
            texture: None,
        };
        let frame = render_2d_or_panic(&[PathRecord::Mesh(mesh)], [0.0, 0.0, 0.0, 1.0]);
        let (left, right) = (pixel(&frame, 64, 26, 32), pixel(&frame, 64, 38, 32));
        assert!(left[0] > 160 && left[2] < 80, "左半应是顶点色红：{left:?}");
        assert!(right[2] > 160 && right[0] < 80, "右半应是顶点色蓝：{right:?}");
    }

    /// **PH Step 3 判据**：上传的程序化 `.hdr` **真的参与着色**。
    ///
    /// 判据不是"函数可见了"，而是：同一块受光网格，只把环境从**内置影棚**
    /// （`BUILTIN_ENVIRONMENT`）换成**上传的自定义图**（上半天亮、下半天黑），
    /// 受光像素均值必须不同 —— 相同就说明链路没通（IBL 的教训刚发生过一次）。
    #[test]
    fn an_uploaded_environment_actually_lights_the_scene() {
        let (w, h) = (64u32, 32u32);
        // RGBE：128 × 2^(e − 136)；e = 131 ⇒ 4.0（天空），e = 0 ⇒ 全黑（地面）。
        let mut rgbe = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let texel = if y < h / 2 {
                [128, 128, 128, 131]
            } else {
                [0, 0, 0, 0]
            };
            for _ in 0..w {
                rgbe.extend_from_slice(&texel);
            }
        }
        let id = 71;
        add_environment(id, w, h, &rgbe).expect("上传自定义环境");

        let mesh = || MeshRecord {
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
            uvs: None,
            normals: None,
            triangles: vec![0, 1, 2, 0, 2, 3],
            fill: [0.8, 0.2, 0.2, 1.0],
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            material: Some([0.0, 0.5, 0.04]),
            colors: None,
            second: None,
            texture: None,
        };
        let frame = |environment: u32| {
            let view = View3 {
                frame: (4.0, 4.0),
                center: [0.0, 0.0, 0.0],
                axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                zoom: 1.0,
                focal: 8.0,
                size: (64, 64),
                background: [0.0, 0.0, 0.0, 1.0],
                light_source: [0.0, 0.0, 1.0],
                exposure: 1.0,
                tone: Some(ze_render_ir::ToneCurve::Clip),
                bloom: 0.0,
                bloom_threshold: 0.0,
                bloom_radius: 0.0,
                ao: None,
                // 只有环境灯：任何亮度差都只能来自环境图。
                lights: vec![Light::Environment { id: environment }],
            };
            render_3d(&view, &[PathRecord::Mesh(mesh())]).expect("3D 渲染失败")
        };
        let mean = |frame: &[u8]| {
            frame
                .chunks_exact(4)
                .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
                .sum::<u64>() as f64
                / (frame.len() / 4) as f64
        };
        let studio = mean(&frame(crate::BUILTIN_ENVIRONMENT));
        let custom = mean(&frame(id));
        assert!(custom > 0.1, "自定义环境必须真的照亮网格（均值 {custom}）");
        assert!(
            (studio - custom).abs() > 1.0,
            "自定义环境与内置影棚必须产生不同光照：studio {studio} vs custom {custom}"
        );
    }

    /// **PH Step 3**：上传入口的错误必须显式（id 0 保留 / 字节数不符）。
    #[test]
    fn environment_upload_errors_are_explicit() {
        assert!(
            add_environment(crate::STUDIO, 1, 1, &[128, 128, 128, 129]).is_err(),
            "id 0 是内置影棚的保留 id"
        );
        let error = add_environment(99, 2, 2, &[0, 0, 0, 0]).unwrap_err();
        assert!(error.contains("字节数"), "{error}");
    }

    /// **PD**：新参数的错误必须显式（不静默修正）。
    #[test]
    fn option_errors_are_explicit() {
        let clear = [0.0, 0.0, 0.0, 1.0];
        let mut one_stop = square8([1.0; 4]);
        one_stop.options.gradient = Some(Gradient {
            stops: vec![[1.0; 4]],
            from: [-8.0, 0.0, 0.0],
            to: [8.0, 0.0, 0.0],
        });
        let error = render_2d(64, 64, 1, clear, &[one_stop.into()]).unwrap_err();
        assert!(error.contains("至少两个色标"), "{error}");

        let mut zero_period = square8([1.0; 4]);
        zero_period.options.dash = Some(Dash {
            period: 0.0,
            duty: 0.5,
            phase: 0.0,
        });
        let error = render_2d(64, 64, 1, clear, &[zero_period.into()]).unwrap_err();
        assert!(error.contains("dash.period"), "{error}");

        let mut backwards = square8([1.0; 4]);
        backwards.options.curve_window = Some([1.0, 1.0]);
        let error = render_2d(64, 64, 1, clear, &[backwards.into()]).unwrap_err();
        assert!(error.contains("curve_window"), "{error}");

        let mut colors = MeshRecord {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uvs: None,
            normals: None,
            triangles: vec![0, 1, 2],
            fill: [1.0; 4],
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            material: None,
            colors: Some(vec![[1.0; 4]]),
            second: None,
            texture: None,
        };
        let error = render_2d(64, 64, 1, clear, &[PathRecord::Mesh(colors.clone())]).unwrap_err();
        assert!(error.contains("逐顶点色"), "{error}");
        colors.colors = None;
        assert!(render_2d(64, 64, 1, clear, &[PathRecord::Mesh(colors)]).is_ok());
    }

    /// **PH Step 1（H-1 判别实验）**：Morph 的三态 `t = 0 / 0.5 / 1` 必须**互异**。
    ///
    /// 判别口径不是"没有报错"，而是像素：
    /// - `t = 0` 必须逐位等于**起点形状**（权重 0 ⇒ 不挂第二项）；
    /// - `t = 0.5` 与两端都不同（真的插值了，不是整项乘 0）；
    /// - `t = 1` 与起点不同（权重生效）。
    ///
    /// 若三态相同，说明 `m2` 没被采样器读进去 —— 与 PG 的 IBL 同型（链路通、整项乘 0），
    /// 那时应该先查编码而不是调参。
    #[test]
    fn morph_mix_changes_the_shape_and_zero_equals_the_start() {
        use ze_notion::{Path2D, Transform2D, Vec2};
        use ze_render_ir::Style2D;

        let square = Path2D::from_polyline(
            &[
                Vec2::new(16.0, 16.0),
                Vec2::new(48.0, 16.0),
                Vec2::new(48.0, 48.0),
                Vec2::new(16.0, 48.0),
            ],
            true,
        );
        let diamond = Path2D::from_polyline(
            &[
                Vec2::new(32.0, 10.0),
                Vec2::new(54.0, 32.0),
                Vec2::new(32.0, 54.0),
                Vec2::new(10.0, 32.0),
            ],
            true,
        );
        let style = Style2D {
            fill: None,
            stroke: Some(ze_notion::Color32::WHITE),
            stroke_width: 4.0,
            ..Default::default()
        };
        let clear = [0.0, 0.0, 0.0, 1.0];
        let at = |from: &Path2D, to: &Path2D, mix: f32| {
            let records =
                crate::scene::morph_records(from, to, mix, &style, Transform2D::identity(), (64, 64))
                    .expect("形变映射");
            render_2d_or_panic(&records, clear)
        };

        let start = at(&square, &square, 0.0);
        let zero = at(&square, &diamond, 0.0);
        let half = at(&square, &diamond, 0.5);
        let one = at(&square, &diamond, 1.0);
        assert_eq!(zero, start, "t = 0 必须逐位等于起点形状（key2 = 0）");

        let changed = |a: &[u8], b: &[u8]| {
            a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x != y).count()
        };
        assert!(changed(&zero, &half) > 100, "t = 0 与 t = 0.5 必须不同");
        assert!(changed(&half, &one) > 100, "t = 0.5 与 t = 1 必须不同");
        assert!(changed(&zero, &one) > 100, "t = 0 与 t = 1 必须不同");
    }

    /// **PH Step 1（H3）**：两端曲线数不一致 ⇒ 接缝层**显式拒绝**，不静默画错。
    #[test]
    fn unaligned_morph_endpoints_are_rejected() {
        use ze_notion::{Path2D, Transform2D, Vec2};
        use ze_render_ir::Style2D;

        let square = Path2D::from_polyline(
            &[
                Vec2::new(16.0, 16.0),
                Vec2::new(48.0, 16.0),
                Vec2::new(48.0, 48.0),
                Vec2::new(16.0, 48.0),
            ],
            true,
        );
        let triangle = Path2D::from_polyline(
            &[
                Vec2::new(32.0, 12.0),
                Vec2::new(52.0, 52.0),
                Vec2::new(12.0, 52.0),
            ],
            true,
        );
        let style = Style2D {
            fill: None,
            stroke: Some(ze_notion::Color32::WHITE),
            stroke_width: 4.0,
            ..Default::default()
        };
        let error = crate::scene::morph_records(
            &square,
            &triangle,
            0.5,
            &style,
            Transform2D::identity(),
            (64, 64),
        )
        .unwrap_err();
        assert!(error.to_string().contains("未对齐"), "{error}");
    }

    /// **E1（阶段 1）**：参数摘要只哈希 `kind + params`，**不哈希求值结果 / 结构坐标**。
    ///
    /// 这是"结构指纹改哈希参数"的可执行判据：同参数的两次求值必须同摘要，
    /// 参数一变摘要就变；几何（`delta`）变但参数不变时摘要**不动**
    /// —— 几何是派生量，不该进跨设备层（GAPS AR-05 / AR-08）。
    #[test]
    fn param_digest_hashes_the_numbers_not_the_geometry() {
        let delta = vec![[1.0, 2.0, 3.0]; 8];
        let other_delta = vec![[9.0, 9.0, 9.0]; 8];
        let a = ParamGeometry::lerp(delta.clone(), 0.25);
        let same = ParamGeometry::lerp(delta, 0.25);
        let moved = ParamGeometry::lerp(other_delta, 0.25);
        let different_mix = ParamGeometry::lerp(vec![[1.0, 2.0, 3.0]; 8], 0.5);
        assert_eq!(a.param_digest(), same.param_digest(), "同参数 ⇒ 同摘要");
        assert_eq!(
            a.param_digest(),
            moved.param_digest(),
            "只改几何（delta）⇒ 摘要不动（几何不进跨设备层）"
        );
        assert_ne!(
            a.param_digest(),
            different_mix.param_digest(),
            "参数变 ⇒ 摘要必须变"
        );
        assert_ne!(
            ParamGeometry::window(0.0, 2.0).param_digest(),
            ParamGeometry::grow(2.0).param_digest(),
            "同数字不同 kind ⇒ 摘要必须不同（域标签）"
        );
    }

    fn four_segment_line() -> ze_notion::Path2D {
        use ze_notion::{Path2D, Vec2};
        Path2D::from_polyline(
            &[
                Vec2::new(8.0, 32.0),
                Vec2::new(20.0, 32.0),
                Vec2::new(32.0, 32.0),
                Vec2::new(44.0, 32.0),
                Vec2::new(56.0, 32.0),
            ],
            false,
        )
    }

    fn stroke_style() -> ze_render_ir::Style2D {
        ze_render_ir::Style2D {
            fill: None,
            stroke: Some(ze_notion::Color32::WHITE),
            stroke_width: 4.0,
            ..Default::default()
        }
    }

    /// **E1**：`Window`（参数化几何）与 `PathOptions::curve_window`（PE 的样式位）
    /// 是同一件事 —— 像素必须逐位相同。
    #[test]
    fn window_param_and_curve_window_render_alike() {
        use crate::scene::param_records;
        use ze_notion::Transform2D;
        use ze_render_ir::PathOptions;

        let path = four_segment_line();
        let style = stroke_style();
        let options = PathOptions {
            curve_window: Some([1.0, 2.0]),
            ..Default::default()
        };
        let via_options = crate::scene::scene_records(
            &[crate::scene::SceneItem {
                shape: &ze_render_ir::Shape2D::Path(path.clone()),
                transform: Transform2D::identity(),
                style: ze_render_ir::Style2D {
                    paint: options,
                    ..style.clone()
                },
            }],
            (64, 64),
        )
        .expect("样式窗口映射");
        let via_param = param_records(
            &path,
            Some(ParamGeometry::window(1.0, 2.0)),
            &style,
            Transform2D::identity(),
            (64, 64),
        )
        .expect("参数窗口映射");
        let clear = [0.0, 0.0, 0.0, 1.0];

        let a = render_2d_or_panic(&via_options, clear);
        let b = render_2d_or_panic(&via_param, clear);
        assert_eq!(a, b, "Window 与 curve_window 必须逐位相同");
    }

    /// **E1**：`Grow(n)` 就是 `Window(0, n)`；`Grow(0)` 整条不可见（与窗口 [0,0] 同口径）。
    #[test]
    fn grow_is_a_window_prefix() {
        let path = four_segment_line();
        let style = stroke_style();
        let clear = [0.0, 0.0, 0.0, 1.0];
        let render = |param: ParamGeometry| {
            let records = crate::scene::param_records(
                &path,
                Some(param),
                &style,
                ze_notion::Transform2D::identity(),
                (64, 64),
            )
            .expect("参数化几何映射");
            render_2d_or_panic(&records, clear)
        };
        let grow_two = render(ParamGeometry::grow(2.0));
        let window_two = render(ParamGeometry::window(0.0, 2.0));
        assert_eq!(grow_two, window_two, "Grow(n) 必须等于 Window(0, n)");

        let full = render(ParamGeometry::grow(4.0));
        let none = render(ParamGeometry::grow(0.0));
        let changed = |a: &[u8], b: &[u8]| {
            a.chunks_exact(4)
                .zip(b.chunks_exact(4))
                .filter(|(x, y)| x != y)
                .count()
        };
        assert!(changed(&none, &full) > 100, "Grow(0) 必须整条不可见");
        assert!(changed(&grow_two, &full) > 50, "窗口收窄必须真的减少可见段");
    }

    /// **E3（阶段 3）**：select 粒度的决策数据 —— 按表达式聚簇 draw（组内保 z 序）
    /// 是否破坏深度合成。
    ///
    /// 实验：同一组不透明方块（z = −1 / 0 / +1）以两种**声明顺序**渲染：
    /// - `declaration`：红(z−1)、绿(z0)、蓝(z+1)；
    /// - `clustered`：先按"表达式"分组（红/蓝属同一表达式，绿属另一条），
    ///   组间按各组的**最小 z** 排、组内保 z 序 ⇒ 红、蓝、绿。
    ///
    /// 判据：像素**逐位相同**。若相同 ⇒ vector 后端本来就是逐像素深度合成，
    /// 按表达式聚簇不会改变结果 ⇒ `uniform select`（per-draw）可行（结局 A）。
    ///
    /// 边界（诚实）：GPU 侧"每次 draw 换表达式"的耗时差**未测** —— 那需要先加
    /// shader 分支（4-A 的 `concat` 接缝）。本实验只给正确性数据与 CPU 分组代价。
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn e3_expression_clustered_draws_keep_depth_compositing() {
        let square = |z: f32, color: [f32; 4]| {
            let mut path = FilledPath::polygon(
                &[
                    [-1.0, -1.0, 0.0],
                    [1.0, -1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [-1.0, 1.0, 0.0],
                ],
                color,
            );
            path.model[2][3] = z;
            PathRecord::Fill(path)
        };
        let red = square(-1.0, [1.0, 0.0, 0.0, 1.0]);
        let green = square(0.0, [0.0, 1.0, 0.0, 1.0]);
        let blue = square(1.0, [0.0, 0.0, 1.0, 1.0]);
        let declaration = vec![red.clone(), green.clone(), blue.clone()];
        // 表达式聚簇：红/蓝同组（expr 0，min z = −1），绿单独（expr 1，z = 0）。
        let clustered = vec![red, blue, green];
        let view = View3 {
            frame: (4.0, 4.0),
            center: [0.0, 0.0, 0.0],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            zoom: 1.0,
            focal: 8.0,
            size: (64, 64),
            background: [0.0, 0.0, 0.0, 1.0],
            light_source: [0.0, 0.0, 1.0],
            exposure: 1.0,
            tone: Some(ze_render_ir::ToneCurve::Clip),
            bloom: 0.0,
            bloom_threshold: 0.0,
            bloom_radius: 0.0,
            ao: None,
            lights: Vec::new(),
        };
        let a = render_3d_with(64, 64, 1, &view, &[], &declaration).expect("声明序渲染");
        let b = render_3d_with(64, 64, 1, &view, &[], &clustered).expect("聚簇序渲染");
        assert_eq!(
            a, b,
            "按表达式聚簇改变了像素 ⇒ uniform select 与深度合成冲突（E3 应判 B/C）"
        );
    }
}
