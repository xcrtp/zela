//! B10 **3D 场景 → 矢量后端**的桥：`Camera3D` → [`View3`]、`LightQueue` → 灯、
//! `DrawQueue::meshes` → [`MeshRecord`]。
//!
//! # 为什么桥在本 crate（B11 Step 5 从 `zela-app` 移入）
//! `zela-render-vector` 是**移植层**，只依赖 kernel（`ze-notion` / `ze-render-ir`），
//! 不认识 `zela-render` 的队列与场景组件（见它的 Cargo.toml 与 B4 验收 §1.3）。
//! 桥接需要两边同时在场；而 native（`zela-app::frame_loop`）与浏览器（`zela-web`）
//! 都要用它 ⇒ 放在两边唯一的公共层 `zela-playback`，避免两份实现漂移。
//!
//! # 口径（全部来自代码事实，不猜）
//! - 相机：右手系，`focal = |eye − target|`，`frame = 2·focal·tan(fov_y/2)`；
//!   矢量 `View3` 只有这种 **focal 透视**形式 ⇒ 正交相机**显式报错**，不近似。
//! - 灯：`LightQueue.direction` **就是"指向光源"**（wgpu 的 `fs_pbr` 直接
//!   `dot(n, dirs[i].xyz)`；B14 实测：旧注释说的"传播方向"是错的，桥里多做了一次取反
//!   ⇒ 矢量侧所有正面都收不到方向光）。矢量 `Light::Sun.direction` 同为"指向光源"
//!   ⇒ **原样透传**。环境项与 wgpu 兜底同值（`DEFAULT_AMBIENT = 0.2`）。
//! - 空灯队列 ⇒ 与 wgpu 的 `default_light_bytes` 同参：方向 `+Z`（指向光源）+ 环境 0.2。
//! - 网格：`Material3D` 没有 reflectance 字段 ⇒ 取 `0.04`（电介质 F0；B14 Step 2 改为
//!   0.04，理由见下方 `DEFAULT_REFLECTANCE` 的注释），该取舍记在 GAPS X1-03。

use ze_notion::{Color32, Mat4};
use ze_render_ir::{AmbientOcclusion, Bloom, RenderParams, ShaderVariant, ToneMapping};
use ze_scene_core::components::{Material3D, MeshGeometry, PrimitiveTopology};
use ze_scene_core::object::{Camera3D, Projection};
use zela_render::draw::DrawQueue;
use zela_render::light_queue::LightQueue;
use zela_render_vector::{Light, MeshRecord, PathRecord, View3};

/// 与 wgpu 兜底一致的环境项（`wgpu_3d.rs` 的 `DEFAULT_AMBIENT`）。
const DEFAULT_AMBIENT: f32 = 0.2;

/// 方向光 / 点光的强度换算（B14 Step 2，X4-02）。
///
/// 两边的直接光项**不是同一个归一化**（都以"白色哑面正对光"为参照）：
/// - wgpu（`cube.wgsl::fs_pbr`）：`(1 − ambient) · I · NdotL · (kd · albedo / π + spec)`；
/// - 矢量（`light.wgsl` 头注）：`I · NdotL` 即"强度 1 = 正对光的白面显示为白"。
/// ⇒ 桥里乘 `(1 − ambient) / π` 才能让漫反射项一致（高光项仍是两套实现，不在此对齐）。
const LIGHT_GAIN: f32 = (1.0 - DEFAULT_AMBIENT) / std::f32::consts::PI;

/// `Material3D` 没有 reflectance 字段时取的常数。
///
/// B14 Step 2：wgpu 的 PBR 用 `f0 = mix(0.04, albedo, metallic)`（电介质 F0 = 0.04），
/// 旧值 0.5（"电介质常用值"）让矢量侧高光强了一个量级 ⇒ 与 wgpu 对拍永远偏亮。
const DEFAULT_REFLECTANCE: f32 = 0.04;

// ─── PB Step 1（V-01）：阴影开关 ──────────────────────────────────────────────

const SHADOWS_UNSET: u8 = 0;
const SHADOWS_ON: u8 = 1;
const SHADOWS_OFF: u8 = 2;

/// 是否让灯投影（0 = 还没问过）。
static SHADOWS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(SHADOWS_UNSET);

/// **PB Step 1（V-01）**：显式开关阴影（宿主用：`ZELA_SHADOWS=0` / 浏览器 `?shadows=0` 关）。
///
/// 为什么先给显式开关再谈默认：开阴影会让 `Buffers` 新增一块 2048²×4 的深度阵列
///（B17 之后随 `Player` 常驻 ⇒ **一次性**，不是每帧），且会改 `[vector]` 基线。
///
/// **PI-3（2026-10-25）已按实测拍板：默认开。** 依据（M-3，`pbr_lights` 320×240）：
/// GPU 每帧 3.645 → 3.604 ms（无可测变化，落在噪声内），资源 39 tex / 34.70 MB →
/// 40 tex / 51.48 MB（**+16.77 MB 一次性**）。⇒ 只付一次显存，不付每帧时间。
pub fn set_shadows(on: bool) {
    SHADOWS.store(
        if on { SHADOWS_ON } else { SHADOWS_OFF },
        std::sync::atomic::Ordering::Relaxed,
    );
    // **PI-04（PC-05）**：阴影状态是 player 缓存的一个维度 —— 让渲染侧推进代数，
    // 下一帧换一个 player（`shadow_maps` 只扩不缩，不能跨状态复用）。
    zela_render_vector::set_shadow_state(on);
}

/// 当前是否投影：默认 **开**（PI-3）；native 首次读取时看 `ZELA_SHADOWS`（`0` ⇒ 关），
/// 浏览器由 [`set_shadows`] 决定。
pub fn shadows() -> bool {
    match SHADOWS.load(std::sync::atomic::Ordering::Relaxed) {
        SHADOWS_ON => true,
        SHADOWS_OFF => false,
        _ => {
            #[cfg(not(target_arch = "wasm32"))]
            let on = std::env::var("ZELA_SHADOWS")
                .map(|value| !matches!(value.trim(), "0" | "off" | "false"))
                .unwrap_or(true);
            #[cfg(target_arch = "wasm32")]
            let on = true;
            SHADOWS.store(
                if on { SHADOWS_ON } else { SHADOWS_OFF },
                std::sync::atomic::Ordering::Relaxed,
            );
            zela_render_vector::set_shadow_state(on);
            on
        }
    }
}

// ─── PG Step 3：IBL（内置影棚）开关 ────────────────────────────────────────────
//
// 场景层没有"用哪个环境"的表达（`ze-scene-*` 只有方向光 / 点光，PG 不碰场景层），
// 所以本批走**宿主开关**（与 PB Step 1 的 `ZELA_SHADOWS` 同一模式）：native 读
// `ZELA_IBL`，浏览器由 [`set_environment`] 决定。
//
// **PI-3（2026-10-25）已按实测拍板：默认开。** 依据（M-4，`pbr_materials` 320×240）：
// GPU 每帧 3.618 → 4.039 ms（**+0.42 ms / +11.6%**），资源**实际** ≈ +35.5 MB
//（记账 +203.4 MB 是 `texture_bytes()` 对 mipmapped 纹理的过计：按 size×mip 级数算，
// 实际约 4/3 ⇒ 计数口径缺口见 GAPS PI-07）。用户已确认接受这个每帧成本。
//
// 影棚本身是**程序化生成的**（`environment.rs::studio()`，1024×512 + 4 个柔光箱），
// 不需要任何 radiance 资产 —— PD-03 里"需要真实资产"的旧成本描述已作废。

const ENVIRONMENT_UNSET: u8 = 0;
const ENVIRONMENT_ON: u8 = 1;
const ENVIRONMENT_OFF: u8 = 2;

static ENVIRONMENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(ENVIRONMENT_UNSET);

/// 显式开关 IBL（宿主用：浏览器 / 测试）。
pub fn set_environment(on: bool) {
    ENVIRONMENT.store(
        if on { ENVIRONMENT_ON } else { ENVIRONMENT_OFF },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// 当前是否给视图加环境光：默认 **开**（PI-3）；native 首次读取时看 `ZELA_IBL`（`0` ⇒ 关）。
pub fn environment() -> bool {
    match ENVIRONMENT.load(std::sync::atomic::Ordering::Relaxed) {
        ENVIRONMENT_ON => true,
        ENVIRONMENT_OFF => false,
        _ => {
            #[cfg(not(target_arch = "wasm32"))]
            let on = std::env::var("ZELA_IBL")
                .map(|value| !matches!(value.trim(), "0" | "off" | "false"))
                .unwrap_or(true);
            #[cfg(target_arch = "wasm32")]
            let on = true;
            ENVIRONMENT.store(
                if on { ENVIRONMENT_ON } else { ENVIRONMENT_OFF },
                std::sync::atomic::Ordering::Relaxed,
            );
            on
        }
    }
}

/// **PG Step 3**：本帧的环境图灯（`Light::Environment`）。
///
/// 两个入口都通向同一条后端链路（`render.rs::with_sun` 惰性造影棚、`light.wgsl` 的
/// `environment_at`）：
/// 1. `RenderParams.env`（IR 既有字段，自定义环境的将来入口）；
/// 2. 宿主开关 ⇒ 内置影棚 [`zela_render_vector::BUILTIN_ENVIRONMENT`]。
pub fn environment_light(params: &RenderParams) -> Option<Light> {
    if let Some(handle) = params.env {
        return Some(Light::Environment {
            id: handle.id() as u32,
        });
    }
    environment().then_some(Light::Environment {
        id: zela_render_vector::BUILTIN_ENVIRONMENT,
    })
}

/// 把 `Camera3D` 分解成矢量后端的 `View3`（透视）。
///
/// 失败条件（都显式，不静默近似）：
/// - 正交投影（`View3` 无正交形式）；
/// - `eye == target`（无视线）或 `up` 与视线平行（look-at 退化）。
pub fn view3(
    camera: &Camera3D,
    size: (u32, u32),
    background: [f32; 4],
    params: &RenderParams,
) -> Result<View3, String> {
    let Projection::Perspective { fov_y, .. } = camera.projection else {
        return Err(
            "矢量后端的 3D 视图只有 focal 透视形式：本帧相机是正交投影（不静默近似）"
                .to_string(),
        );
    };
    let to_eye = camera.eye - camera.target;
    let focal = to_eye.length();
    if focal < 1e-6 {
        return Err(format!(
            "3D 相机退化：eye 与 target 重合（focal = {focal}）"
        ));
    }
    let toward_eye = to_eye / focal;
    let forward = -toward_eye;
    let right = forward.cross(camera.up);
    let right_len = right.length();
    if right_len < 1e-6 {
        return Err("3D 相机退化：up 与视线平行（look-at 无解）".to_string());
    }
    let right = right / right_len;
    let up = right.cross(forward);

    let aspect = size.0 as f32 / size.1.max(1) as f32;
    let frame_h = 2.0 * focal * (fov_y * 0.5).tan();
    let frame_w = frame_h * aspect;

    let (exposure, tone) = match params.tone {
        Some(ToneMapping { exposure, curve }) => (exposure, Some(curve)),
        None => (1.0, None),
    };
    // PB Step 2（V-02）：三个字段**全部**接上 —— intensity 是扩散比例，threshold 是
    // 参与扩散的亮度门槛，radius 决定金字塔最粗一级（扩散半径，屏幕像素）。
    let (bloom, bloom_threshold, bloom_radius) = match params.bloom {
        Some(Bloom {
            threshold,
            intensity,
            radius,
        }) => (intensity, threshold, radius),
        None => (0.0, 0.0, 0.0),
    };
    let ao = params
        .ao
        .map(|AmbientOcclusion { radius, intensity }| (intensity, radius));

    Ok(View3 {
        frame: (frame_w, frame_h),
        center: [camera.target.x, camera.target.y, camera.target.z],
        axes: [
            [right.x, right.y, right.z],
            [up.x, up.y, up.z],
            [toward_eye.x, toward_eye.y, toward_eye.z],
        ],
        zoom: 1.0,
        focal,
        size,
        background,
        // 只在没有显式灯时被上游用于记录；这里给一个稳定的世界方向。
        light_source: [0.0, -1.0, 0.0],
        exposure,
        tone,
        bloom,
        bloom_threshold,
        bloom_radius,
        ao,
        lights: Vec::new(),
    })
}

/// `LightQueue` → 矢量灯列表（方向取反 + 环境项与 wgpu 兜底同值）。
pub fn lights(queue: Option<&LightQueue>) -> Vec<Light> {
    let mut out = Vec::new();
    if let Some(queue) = queue {
        out.extend(queue.directional.iter().map(|light| Light::Sun {
            // B14：两边的 `direction` 都是"指向光源" ⇒ **不取反**（取反会让正面全黑）。
            direction: [light.direction.x, light.direction.y, light.direction.z],
            color: [light.color[0], light.color[1], light.color[2]],
            intensity: light.intensity * LIGHT_GAIN,
            shadows: shadows(),
        }));
        out.extend(queue.points.iter().map(|light| Light::Point {
            at: [light.position.x, light.position.y, light.position.z],
            color: [light.color[0], light.color[1], light.color[2]],
            intensity: light.intensity * LIGHT_GAIN,
            reach: light.range,
        }));
    }
    if out.is_empty() {
        // 与 wgpu 的 `default_light_bytes` 同参：方向 `+Z`（**指向光源**）。
        out.push(Light::Sun {
            direction: [0.0, 0.0, 1.0],
            color: [1.0, 1.0, 1.0],
            intensity: LIGHT_GAIN,
            shadows: shadows(),
        });
    }
    // 环境项在 wgpu 侧**恒**为 `DEFAULT_AMBIENT`（空队列也一样）⇒ 这里保持一致；
    // 它同时是 AO 生效的前提（`occlusion_of` 要求 kind 0 或 4）。
    out.push(Light::Ambient {
        color: [1.0, 1.0, 1.0],
        intensity: DEFAULT_AMBIENT,
    });
    out
}

/// 本帧的全部 3D 网格 → 矢量记录（顺序 = 队列顺序）。
///
/// **PH Step 2**：`lines` 给出线框细条的朝向与宽度。后端没有线框路径
/// （`MeshForm::Line` 零命中、`vector3d.rs` 对非 Tri 显式 `Err`）⇒ 在**接缝层**
/// 把 `Lines` 转成细条三角形。宽度是 1 屏幕像素（与 wgpu 的 `LineList` 同口径，
/// 见 [`LineStyle`]）。
pub fn mesh_records(queue: &DrawQueue, lines: LineStyle) -> Result<Vec<PathRecord>, String> {
    let mut out = Vec::new();
    for (index, draw) in queue.meshes().enumerate() {
        let record = mesh_record(&draw.geometry, draw.transform, &draw.material, lines)
            .map_err(|error| format!("3D 网格 #{index}：{error}"))?;
        if let Some(record) = record {
            out.push(record);
        }
    }
    Ok(out)
}

/// **PH Step 2**：线框细条的朝向与宽度。
///
/// `world_per_pixel` = `view.frame.1 / height`（焦距平面处 1 屏幕像素对应的世界长度）。
/// 场景层没有 `line_width` 字段（`MeshGeometry` 只有 positions / normals / uvs /
/// indices / topology）⇒ **宽度不可由作者控制**；这里取 1 px 与 wgpu 的
/// `LineList`（不支持 linewidth）同口径，并记账（GAPS PH-06）。
#[derive(Debug, Clone, Copy)]
pub struct LineStyle {
    /// 相机朝向（世界坐标，"指向眼睛"）：细条垂直于它展开，正面朝向相机。
    pub toward_eye: [f32; 3],
    pub world_per_pixel: f32,
}

/// 一条网格：几何 + 模型矩阵 + 材质。
fn mesh_record(
    geometry: &MeshGeometry,
    transform: Mat4,
    material: &Material3D,
    lines: LineStyle,
) -> Result<Option<PathRecord>, String> {
    let (positions, normals, triangles, model) = match geometry.topology {
        PrimitiveTopology::Triangles => {
            let positions: Vec<[f64; 3]> = geometry
                .positions
                .iter()
                .map(|p| [p.x as f64, p.y as f64, p.z as f64])
                .collect();
            let triangles = match &geometry.indices {
                Some(indices) => indices.clone(),
                None => (0..positions.len() as u32).collect(),
            };
            let normals = geometry.normals.as_ref().map(|normals| {
                normals
                    .iter()
                    .map(|n| [n.x as f64, n.y as f64, n.z as f64])
                    .collect::<Vec<_>>()
            });
            (positions, normals, triangles, mat34(transform))
        }
        // **PH Step 2**：线框 → 细条三角形（后端无 `MeshForm::Line` 路径）。
        PrimitiveTopology::Lines => {
            if material.texture.is_some() {
                return Err(
                    "线框网格带材质纹理：细条没有可用的 UV（不静默贴错，见 GAPS PH-06）"
                        .to_string(),
                );
            }
            let (positions, normals, triangles) =
                line_ribbons(&geometry.positions, geometry.indices.as_deref(), transform, lines)?;
            if triangles.is_empty() {
                return Ok(None); // 全部线段退化为点（与视线平行）⇒ 没有可画的面积
            }
            (positions, Some(normals), triangles, IDENTITY_3X4)
        }
        PrimitiveTopology::Points => {
            return Err(
                "点拓扑（PrimitiveTopology::Points）尚未映射：矢量侧的点云是 `PointsRecord`，\
                 网格记录不接受 Points（不静默丢）"
                    .to_string(),
            );
        }
    };
    if positions.is_empty() {
        return Err("几何没有顶点".to_string());
    }
    let uvs = match geometry.topology {
        // **PH Step 2**：细条的顶点是接缝层生成的（每段 4 个），原 UV 的逐顶点对应关系
        // 不再成立；带纹理的线框已在上面的分支显式拒绝，这里也不附加错位的 UV。
        PrimitiveTopology::Lines => None,
        _ => match &geometry.uvs {
            Some(uvs) => Some(
                uvs.iter()
                    .map(|uv| [uv.x as f64, uv.y as f64])
                    .collect::<Vec<_>>(),
            ),
            // **PG Step 2**：几何没有 UV 时按 `uv_projection` 兜底 —— 与 wgpu 侧
            // `zela_render::vertex_record::mesh_vertex_buffer` 用**同一个函数**，
            // 否则同一张纹理在两个后端上会贴成不同的样子（Planar 的 `1 − y` 尤其）。
            None => material.texture.map(|_| {
                zela_render::vertex_record::fallback_uvs(
                    &geometry.positions,
                    material.uv_projection,
                )
                .into_iter()
                .map(|uv| [uv.x as f64, uv.y as f64])
                .collect::<Vec<_>>()
            }),
        },
    };
    // **PF Step 1**：`ShaderVariant` 必须映射，不能一律按 PBR 受光。
    //
    // 实测（PF §2 E-2 的追加判别）：修前 Flat / Lambert / Pbr 三个变体在矢量侧
    // 输出**逐位相同**（`d2cf72af…` ×3），而 wgpu 是三条不同哈希 ⇒ 变体在桥里被吞掉。
    // fusion 的两个半透明面板正是 `Flat`（无光照）⇒ 被 PBR 照亮后面部发黑。
    //
    // 映射口径（后端只有一个 PBR 材质模型，不新增后端行为）：
    // - `Flat`   ⇒ **无材质**（`material[3] = 0`，平涂，与 `fs_flat` 同语义）；
    // - `Lambert`⇒ 纯漫反射近似：金属度 0、粗糙度 1（GGX 的镜面项随粗糙度趋于 0）；
    // - `Pbr`    ⇒ 原样透传（金属度 / 粗糙度）。
    let material_params = match material.variant {
        ShaderVariant::Flat => None,
        ShaderVariant::Lambert => Some([0.0, 1.0, DEFAULT_REFLECTANCE]),
        ShaderVariant::Pbr => Some([
            material.metallic,
            material.roughness,
            DEFAULT_REFLECTANCE,
        ]),
    };
    // **PF Step 1**：`transparent` 意图映射。
    //
    // 矢量后端没有"透明桶"位 —— 它是每像素链表 + 按深度合成的 OIT，alpha **本来就**
    // 参与混合（不需要新增后端行为）。需要映射的是**反面**：`transparent = false` 时
    // wgpu 用 `LayerBlend::Opaque`（REPLACE），颜色 alpha 不参与合成 ⇒ 视觉上是不透明。
    // 这里把 `fill.a` 抬到 1 以表达同一意图；`transparent = true` 原样保留 alpha。
    //
    // ⚠️ `double_sided` 在矢量侧**不可表达**（后端没有 culling 开关）：双面都画。
    // 不透明网格两面深度遮挡 ⇒ 无差别；**半透明 + double_sided = false** 会多出背面
    // （已记 PF-04，不静默）。
    let mut fill = rgba(material.color);
    if !material.transparent {
        fill[3] = 1.0;
    }
    Ok(Some(PathRecord::Mesh(MeshRecord {
        positions,
        uvs,
        normals,
        triangles,
        fill,
        model,
        material: material_params,
        colors: None,
        second: None,
        // **PG Step 2**：材质纹理 → 记录的纹理来源。句柄 id 由场景层稳定派生
        // （内容层 = `content_texture_handle(surface)`），主帧按同一个 key 采样相机视图。
        texture: material
            .texture
            .map(|handle| zela_render_vector::TextureSource::View(handle.id())),
    })))
}

/// **PH Step 2**：把 `LineList`（每 2 个顶点一段）扩成**细条三角形**。
///
/// 每段生成一条垂直于视线、宽 `1 px` 的矩形带（4 顶点 / 2 三角形），法线取
/// "指向眼睛" ⇒ 与同一相机下的三角形面同一套光照口径。
///
/// 顶点输出为**世界坐标**（`model = 恒等`），这样宽度不必随模型缩放换算，
/// 朝向也只需一次世界空间的 `cross`。
fn line_ribbons(
    positions: &[ze_notion::Vec3],
    indices: Option<&[u32]>,
    transform: Mat4,
    lines: LineStyle,
) -> Result<(Vec<[f64; 3]>, Vec<[f64; 3]>, Vec<u32>), String> {
    let pairs: Vec<[u32; 2]> = match indices {
        Some(indices) => {
            if indices.len() % 2 != 0 {
                return Err(format!(
                    "线框索引必须是偶数（每 2 个一段），实际 {}",
                    indices.len()
                ));
            }
            indices.chunks_exact(2).map(|pair| [pair[0], pair[1]]).collect()
        }
        None => (0..positions.len() / 2)
            .map(|i| [2 * i as u32, 2 * i as u32 + 1])
            .collect(),
    };
    let toward_eye = {
        let v = ze_notion::Vec3::from_array(lines.toward_eye);
        if v.length_squared() > 1e-12 {
            v.normalize()
        } else {
            ze_notion::Vec3::Z
        }
    };
    let half = 0.5 * lines.world_per_pixel.max(1e-6);
    let mut points: Vec<[f64; 3]> = Vec::with_capacity(pairs.len() * 4);
    let mut normals: Vec<[f64; 3]> = Vec::with_capacity(pairs.len() * 4);
    let mut triangles: Vec<u32> = Vec::with_capacity(pairs.len() * 6);
    for [a, b] in pairs {
        let (Some(a), Some(b)) = (
            positions.get(a as usize).copied(),
            positions.get(b as usize).copied(),
        ) else {
            return Err(format!(
                "线框索引越界（{a} / {b}，顶点数 {}）",
                positions.len()
            ));
        };
        let (a, b) = (transform.transform_point3(a), transform.transform_point3(b));
        let along = b - a;
        let side = along.cross(toward_eye);
        if side.length_squared() <= 1e-12 {
            continue; // 与视线平行 ⇒ 投影成一个点，没有可画的面积
        }
        let side = side.normalize() * half;
        let base = points.len() as u32;
        points.extend_from_slice(&[
            to_f64(a - side),
            to_f64(a + side),
            to_f64(b - side),
            to_f64(b + side),
        ]);
        let normal = to_f64(toward_eye);
        normals.extend_from_slice(&[normal; 4]);
        triangles.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }
    Ok((points, normals, triangles))
}

fn to_f64(v: ze_notion::Vec3) -> [f64; 3] {
    [v.x as f64, v.y as f64, v.z as f64]
}

const IDENTITY_3X4: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

fn rgba(color: Color32) -> [f32; 4] {
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    ]
}

/// glam `Mat4`（列主序）→ 上游 `Mat34`（3×4 行主序）。
fn mat34(m: Mat4) -> [[f32; 4]; 3] {
    let cols = m.to_cols_array_2d();
    [
        [cols[0][0], cols[1][0], cols[2][0], cols[3][0]],
        [cols[0][1], cols[1][1], cols[2][1], cols[3][1]],
        [cols[0][2], cols[1][2], cols[2][2], cols[3][2]],
    ]
}

/// 供调用方断言"本帧是否真的有 3D 内容"（空网格 ⇒ 不是 3D 绘制）。
pub fn has_meshes(queue: &DrawQueue) -> bool {
    queue.meshes().next().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_notion::Vec3;
    use ze_render_ir::Bloom;
    use ze_scene_core::components::PrimitiveTopology;

    fn camera(projection: Projection) -> Camera3D {
        Camera3D {
            eye: Vec3::new(0.0, 0.0, 8.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection,
        }
    }

    fn params(tone: bool) -> RenderParams {
        let mut params = RenderParams::default();
        if tone {
            params.tone = Some(ToneMapping {
                exposure: 2.0,
                curve: ze_render_ir::ToneCurve::AgX,
            });
            params.bloom = Some(Bloom::default());
            params.ao = Some(AmbientOcclusion::default());
        }
        params
    }

    /// 透视相机分解：眼在 +Z ⇒ 朝外轴 +Z、右 +X、上 +Y，focal = 8。
    #[test]
    fn perspective_camera_decomposes_into_view3() {
        let view = view3(
            &camera(Projection::perspective(0.7, 4.0 / 3.0, 0.1, 100.0)),
            (64, 48),
            [0.0, 0.0, 0.0, 1.0],
            &params(false),
        )
        .expect("透视相机可分解");
        assert_eq!(view.focal, 8.0);
        assert_eq!(view.center, [0.0, 0.0, 0.0]);
        assert_eq!(view.axes[0], [1.0, 0.0, 0.0], "right = +X");
        assert_eq!(view.axes[1], [0.0, 1.0, 0.0], "up = +Y");
        assert_eq!(view.axes[2], [0.0, 0.0, 1.0], "toward eye = +Z");
        let expect_h = 2.0 * 8.0 * (0.7f32 * 0.5).tan();
        assert!((view.frame.1 - expect_h).abs() < 1e-4);
        assert!((view.frame.0 - expect_h * (64.0 / 48.0)).abs() < 1e-4);
        assert_eq!(view.tone, None, "默认不开 tone");
        assert_eq!(view.bloom, 0.0);
    }

    /// 着色栈参数从 `RenderParams` 透传（tone 曲线 + 曝光 / bloom 强度 / AO）。
    #[test]
    fn shading_params_pass_through() {
        let view = view3(
            &camera(Projection::perspective(0.7, 1.0, 0.1, 100.0)),
            (64, 64),
            [0.0; 4],
            &params(true),
        )
        .unwrap();
        assert_eq!(view.tone, Some(ze_render_ir::ToneCurve::AgX));
        assert_eq!(view.exposure, 2.0);
        assert!(view.bloom > 0.0);
        assert!(view.ao.is_some());
    }

    /// 正交相机与退化 look-at **显式报错**（不静默近似）。
    #[test]
    fn unsupported_cameras_fail_explicitly() {
        let ortho = camera(Projection::orthographic(2.0, 1.0, 0.1, 100.0));
        let error = view3(&ortho, (64, 64), [0.0; 4], &params(false)).unwrap_err();
        assert!(error.contains("正交"), "{error}");

        let mut degenerate = camera(Projection::perspective(0.7, 1.0, 0.1, 100.0));
        degenerate.target = degenerate.eye;
        let error = view3(&degenerate, (64, 64), [0.0; 4], &params(false)).unwrap_err();
        assert!(error.contains("退化"), "{error}");

        let mut parallel_up = camera(Projection::perspective(0.7, 1.0, 0.1, 100.0));
        parallel_up.up = Vec3::Z;
        let error = view3(&parallel_up, (64, 64), [0.0; 4], &params(false)).unwrap_err();
        assert!(error.contains("平行"), "{error}");
    }

    /// 灯（B14）：方向**原样透传**（两边都是"指向光源"），强度按 `(1−ambient)/π` 换算；
    /// 恒定带 0.2 环境项；空队列给 wgpu 同款兜底。
    #[test]
    fn lights_pass_direction_through_and_scale_intensity() {
        let mut queue = LightQueue::new();
        queue.directional.push(
            zela_render::light_queue::DirectionalLightRecord::new(
                Vec3::new(0.0, 0.0, 1.0),
                Color32::WHITE,
                2.0,
            ),
        );
        queue.points.push(zela_render::light_queue::PointLightRecord::new(
            Vec3::new(1.0, 2.0, 3.0),
            Color32::WHITE,
            1.0,
            5.0,
        ));
        let mapped = lights(Some(&queue));
        assert_eq!(mapped.len(), 3, "方向光 + 点光 + 环境项");
        match mapped[0] {
            Light::Sun { direction, intensity, .. } => {
                assert_eq!(direction, [0.0, 0.0, 1.0], "方向是\"指向光源\"⇒ 原样透传");
                assert!((intensity - 2.0 * LIGHT_GAIN).abs() < 1e-6);
            }
            other => panic!("第一盏应是 Sun：{other:?}"),
        }
        match mapped[2] {
            Light::Ambient { intensity, .. } => assert_eq!(intensity, DEFAULT_AMBIENT),
            other => panic!("最后一盏应是环境项：{other:?}"),
        }

        let empty = lights(None);
        assert_eq!(empty.len(), 2, "空队列 = 默认方向光 + 环境项");
        match empty[0] {
            Light::Sun { direction, .. } => {
                assert_eq!(direction, [0.0, 0.0, 1.0], "与 wgpu default_light_bytes 同参")
            }
            other => panic!("{other:?}"),
        }
    }

    /// **PH Step 2**：线框细条的朝向与宽度（测试默认：看向 +Z、1 世界单位 = 1 px）。
    fn line_style() -> LineStyle {
        LineStyle {
            toward_eye: [0.0, 0.0, 1.0],
            world_per_pixel: 1.0,
        }
    }

    /// 网格：无索引 ⇒ 顺序三角形；无 UV / 法线 ⇒ 留 `None`（上游补零 / 求法线）。
    #[test]
    fn mesh_record_maps_geometry_and_material() {
        let geometry = MeshGeometry::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            None,
            PrimitiveTopology::Triangles,
        );
        let mut material = Material3D::color(Color32::RED);
        material.variant = ShaderVariant::Pbr;
        material.metallic = 0.25;
        material.roughness = 0.75;
        let record = mesh_record(&geometry, Mat4::IDENTITY, &material, line_style())
            .expect("可映射")
            .expect("三角形网格必须产出记录");
        let PathRecord::Mesh(mesh) = record else {
            panic!("应是网格记录");
        };
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.triangles, vec![0, 1, 2], "无索引 ⇒ 顺序");
        assert!(mesh.uvs.is_none() && mesh.normals.is_none());
        assert_eq!(mesh.fill, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(mesh.material, Some([0.25, 0.75, DEFAULT_REFLECTANCE]));
        mesh.validate().expect("映射结果必须自洽");
    }

    /// **PF Step 1（可证伪）**：`ShaderVariant` 必须映射 —— 修前三者逐位相同
    /// （实测 `d2cf72af…` ×3），修后 `Flat` 无材质、`Lambert` 纯漫反射、`Pbr` 透传。
    #[test]
    fn shader_variant_maps_to_material_semantics() {
        let geometry = MeshGeometry::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            None,
            PrimitiveTopology::Triangles,
        );
        let material_of = |variant: ShaderVariant| {
            let mut material = Material3D::color(Color32::RED);
            material.variant = variant;
            material.metallic = 0.25;
            material.roughness = 0.75;
            let PathRecord::Mesh(mesh) =
                mesh_record(&geometry, Mat4::IDENTITY, &material, line_style())
                    .expect("可映射")
                    .expect("三角形网格必须产出记录")
            else {
                panic!("应是网格记录");
            };
            mesh.material
        };
        assert_eq!(
            material_of(ShaderVariant::Flat),
            None,
            "Flat 必须无材质（平涂，与 fs_flat 同语义）"
        );
        assert_eq!(
            material_of(ShaderVariant::Lambert),
            Some([0.0, 1.0, DEFAULT_REFLECTANCE]),
            "Lambert = 纯漫反射近似（忽略作者的 metallic / roughness）"
        );
        assert_eq!(
            material_of(ShaderVariant::Pbr),
            Some([0.25, 0.75, DEFAULT_REFLECTANCE]),
            "Pbr 原样透传"
        );
    }

    /// **PF Step 1**：`transparent = false` ⇒ 抬到不透明（wgpu 的 REPLACE 语义）；
    /// `transparent = true` ⇒ 保留 alpha（矢量 OIT 本来就会按深度混合）。
    #[test]
    fn transparency_intent_maps_to_alpha() {
        let geometry = MeshGeometry::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            None,
            PrimitiveTopology::Triangles,
        );
        let alpha_of = |transparent: bool| {
            let mut material = Material3D::color(Color32::from_rgba(255, 90, 90, 140));
            material.variant = ShaderVariant::Flat;
            material.transparent = transparent;
            let PathRecord::Mesh(mesh) =
                mesh_record(&geometry, Mat4::IDENTITY, &material, line_style())
                    .expect("可映射")
                    .expect("三角形网格必须产出记录")
            else {
                panic!("应是网格记录");
            };
            mesh.fill[3]
        };
        assert!(
            (alpha_of(false) - 1.0).abs() < 1e-6,
            "不透明意图 ⇒ alpha 抬到 1（wgpu 的 REPLACE 不消费颜色 alpha）"
        );
        assert!(
            (alpha_of(true) - 140.0 / 255.0).abs() < 1e-6,
            "透明意图 ⇒ 保留作者的 alpha（OIT 按深度混合）"
        );
    }

    /// **PH Step 2（H-2a/H-2b）**：`Lines` 不再报错 —— 接缝层把它转成细条三角形。
    ///
    /// - H-2a：后端没有线框路径（`MeshForm` 只有 `Tri` / `Line`，矢量 mesh 分支只接受
    ///   三角形），所以转换必须在这一层做；
    /// - H-2b：场景层**没有 `line_width`**（`MeshGeometry` 无该字段）⇒ 宽度取 1 屏幕像素，
    ///   由 [`LineStyle::world_per_pixel`] 换成世界长度。宽度不可由作者控制，已记账。
    #[test]
    fn line_topology_becomes_a_thin_ribbon() {
        // 一条沿 +X 的线段，宽 2 世界单位（world_per_pixel = 2 ⇒ 半宽 1）。
        let geometry = MeshGeometry::new(
            vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)],
            Some(vec![0, 1]),
            PrimitiveTopology::Lines,
        );
        let style = LineStyle {
            toward_eye: [0.0, 0.0, 1.0],
            world_per_pixel: 2.0,
        };
        let PathRecord::Mesh(mesh) =
            mesh_record(&geometry, Mat4::IDENTITY, &Material3D::color(Color32::WHITE), style)
                .expect("线框应可映射")
                .expect("非退化线段必须产出记录")
        else {
            panic!("应是网格记录");
        };
        assert_eq!(mesh.positions.len(), 4, "每段 4 个角点");
        assert_eq!(mesh.triangles, vec![0, 1, 2, 1, 3, 2], "每段 2 个三角形");
        // 展开方向垂直于视线：沿 +X 的线在看向 +Z 时向 ±Y 展开半宽 1。
        let ys: Vec<f64> = mesh.positions.iter().map(|p| p[1]).collect();
        assert!(ys.iter().any(|y| (*y - 1.0).abs() < 1e-9), "{ys:?}");
        assert!(ys.iter().any(|y| (*y + 1.0).abs() < 1e-9), "{ys:?}");
        assert!(
            mesh.normals.as_ref().is_some_and(|n| n.iter().all(|n| n[2] > 0.99)),
            "细条法线应指向相机"
        );
        mesh.validate().expect("细条记录必须自洽");
    }

    /// 点拓扑暂未映射 ⇒ **显式报错**（不静默丢，H3）。
    #[test]
    fn point_topology_fails_explicitly() {
        let geometry =
            MeshGeometry::new(vec![Vec3::ZERO, Vec3::X], None, PrimitiveTopology::Points);
        let error = mesh_record(
            &geometry,
            Mat4::IDENTITY,
            &Material3D::color(Color32::WHITE),
            line_style(),
        )
        .unwrap_err();
        assert!(error.contains("Points"), "{error}");
    }
}
