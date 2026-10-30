// vertex_record.rs — 顶点**生产端**（B3-0：生产逻辑离开 backend crate）。
//
// # 定位
// 本模块只做 CPU 侧工作：几何 → 交错字节（`VertexBufferData`）。它**不依赖 wgpu**，
// 因此任何后端（wgpu / 未来 WebGL / 软件光栅）都复用同一份生产逻辑：
//
// ```text
// 生产端（本模块）：Shape2D / MeshGeometry / LineDraw → VertexBufferData
// 后端          ：VertexBufferData → create_buffer_init（只会上传）
// ```
//
// # 与相机的关系（不变）
// 2D 三角化的容差依赖相机 scale（`tolerance_for_scale`）。本模块只把"调用 + 打包"
// 从后端搬出来，**调用时机仍在渲染阶段**（相机已确定），不提前到场景录制。
//
// # 唯一来源
// - 2D：顶点格式由 `tessellate` 产出的 `VertexLayout` 声明（stride 24）；
// - 3D：布局由**调和**决定 —— shader 需求（`ze_render_ir::vs_requirements_for`）
//   × 几何实际提供的槽经 `ze_vertex::reconcile` 对账，缺槽按默认值表补齐；
//   三角形拓扑下等价于 `VertexLayout::standard_3d()`（stride 32）；
// - 打包后的字节必须满足 `VertexBufferData::validate()`（长度 / 索引范围）。

use ze_notion::{Path2D, Transform2D, Vec2, Vec3};
use ze_render_ir::{PrimitiveTopology, Shape2D, Style2D, UvProjection};
use ze_vertex::{
    ReconcilePlan, Semantic, VertexAttrib, VertexBufferData, VertexBufferError, VertexLayout,
    bytes_of, reconcile,
};
use ze_scene_core::components::MeshGeometry;

use crate::tessellate::{TessKind, Tessellation2D, TessellateError, tessellate, tessellate_reveal};

/// 生产端错误：三角化失败与打包失败都**显式**冒泡（不静默）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VertexRecordError {
    #[error("三角化失败：{0}")]
    Tessellation(#[from] TessellateError),
    #[error("顶点缓冲打包失败：{0}")]
    Buffer(#[from] VertexBufferError),
    #[error(
        "形变两端拓扑不一致（等点重采样后不应发生）：顶点 {from_vertices} vs {to_vertices}、索引 {from_indices} vs {to_indices}"
    )]
    MorphTopologyMismatch {
        from_vertices: usize,
        to_vertices: usize,
        from_indices: usize,
        to_indices: usize,
    },
}

/// 一次三角化 + 打包的产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TessellatedBuffer {
    pub data: VertexBufferData,
    pub kind: TessKind,
}

fn pack(t: Tessellation2D) -> Result<TessellatedBuffer, VertexRecordError> {
    let data = VertexBufferData::new(t.layout, bytes_of(&t.vertices).to_vec(), t.indices)?;
    Ok(TessellatedBuffer {
        data,
        kind: t.kind,
    })
}

/// 2D 图元 → 顶点缓冲（填充 / 描边各至多一项；`Group` 递归成多项）。
pub fn tessellate_to_buffers(
    shape: &Shape2D,
    transform: &Transform2D,
    style: &Style2D,
    tolerance: f32,
) -> Result<Vec<TessellatedBuffer>, VertexRecordError> {
    let mut out = Vec::new();
    for item in tessellate(shape, transform, style, tolerance)? {
        out.push(pack(item)?);
    }
    Ok(out)
}

/// 形变端点：路径 → 顶点缓冲（与 [`tessellate_to_buffers`] 同一份打包）。
pub fn tessellate_path_to_buffers(
    path: &Path2D,
    transform: &Transform2D,
    style: &Style2D,
    tolerance: f32,
) -> Result<Vec<TessellatedBuffer>, VertexRecordError> {
    tessellate_to_buffers(&Shape2D::Path(path.clone()), transform, style, tolerance)
}

/// 书写动画：路径按 `step` 预细分 → 顶点缓冲（`None` = 非描边/无几何）。
pub fn tessellate_reveal_to_buffer(
    path: &Path2D,
    transform: &Transform2D,
    style: &Style2D,
    tolerance: f32,
    step: f32,
) -> Result<Option<TessellatedBuffer>, VertexRecordError> {
    match tessellate_reveal(path, transform, style, tolerance, step) {
        Some(item) => Ok(Some(pack(item)?)),
        None => Ok(None),
    }
}

/// 形变两端的顶点缓冲（拓扑一致 ⇒ 共用索引表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphEndpointBuffers {
    pub from: TessellatedBuffer,
    pub to: TessellatedBuffer,
}

/// 形变端点生产：`from` / `to` 两条路径 ⇒ 两端缓冲。
///
/// - 两侧都无几何 ⇒ `Ok(None)`（调用方跳过该项）；
/// - 仅一侧有几何 ⇒ 用**可三角化的一侧**的拓扑合成退化侧（顶点塌到该路径首点、
///   法线归零，`t = 0` 时三角形退化不可见）；
/// - 拓扑不一致 ⇒ **显式错误**（等点重采样后不应发生，不静默跳过）。
pub fn morph_endpoint_buffers(
    from_path: &Path2D,
    to_path: &Path2D,
    transform: &Transform2D,
    style: &Style2D,
    tolerance: f32,
) -> Result<Option<MorphEndpointBuffers>, VertexRecordError> {
    let from_tess = tessellate(&Shape2D::Path(from_path.clone()), transform, style, tolerance)?
        .into_iter()
        .next();
    let to_tess = tessellate(&Shape2D::Path(to_path.clone()), transform, style, tolerance)?
        .into_iter()
        .next();

    let (from, to) = match (from_tess, to_tess) {
        (Some(f), Some(t)) => (f, t),
        (None, Some(t)) => {
            let c = collapsed_like(&t, from_path);
            (c, t)
        }
        (Some(f), None) => {
            let c = collapsed_like(&f, to_path);
            (f, c)
        }
        (None, None) => return Ok(None),
    };

    if from.indices.len() != to.indices.len() || from.vertices.len() != to.vertices.len() {
        return Err(VertexRecordError::MorphTopologyMismatch {
            from_vertices: from.vertices.len(),
            to_vertices: to.vertices.len(),
            from_indices: from.indices.len(),
            to_indices: to.indices.len(),
        });
    }

    Ok(Some(MorphEndpointBuffers {
        from: pack(from)?,
        to: pack(to)?,
    }))
}

/// 按模板拓扑合成"退化侧"：顶点全部塌到 `zero_path` 的首点，法线归零。
fn collapsed_like(template: &Tessellation2D, zero_path: &Path2D) -> Tessellation2D {
    let pivot = zero_path
        .flatten(0.0)
        .first()
        .copied()
        .unwrap_or(Vec2::ZERO);
    let mut out = template.clone();
    for v in &mut out.vertices {
        v.position = [pivot.x, pivot.y];
        v.normal = [0.0, 0.0];
    }
    out
}

// ─── 3D 生产（B3-0b / 顶点契约扩展）────────────────────────────────────────

/// 提供方布局：几何**实际给了**哪些 3D 槽。
///
/// 位置与 UV 恒在（UV 要么几何自带、要么由 [`fallback_uvs`] 生成），法线可选 ——
/// 这正是"mesh 给什么"的口径，交给 [`reconcile`] 与 shader 的需求对账。
fn provided_3d(has_normals: bool) -> VertexLayout {
    let mut attribs = vec![
        VertexAttrib {
            semantic: Semantic::Position.as_str(),
            ty: Semantic::Position.padding_type(),
            offset: 0,
        },
        VertexAttrib {
            semantic: Semantic::Uv.as_str(),
            ty: Semantic::Uv.padding_type(),
            offset: 0,
        },
    ];
    if has_normals {
        attribs.push(VertexAttrib {
            semantic: Semantic::ShadingNormal.as_str(),
            ty: Semantic::ShadingNormal.padding_type(),
            offset: 0,
        });
    }
    let stride = VertexLayout::compute_layout(&mut attribs);
    VertexLayout {
        attribs,
        stride,
        // 缓存策略 / 步进模式 / AA 模型沿用 3D 标准布局的声明。
        ..VertexLayout::standard_3d()
    }
}

/// 3D 顶点调和：shader 需求（按拓扑选入口）× mesh 提供 → 采纳布局 + 补齐清单。
///
/// 调和失败**显式报错**（缺位置等没有默认值的槽）：猜一个值出来会让像素静默变化。
fn reconcile_3d(topology: PrimitiveTopology, has_normals: bool) -> ReconcilePlan {
    let requirements = ze_render_ir::vs_requirements_for(topology);
    reconcile(&requirements.semantics, &provided_3d(has_normals))
        .unwrap_or_else(|error| panic!("不静默：3D 顶点调和失败：{error}"))
}

/// 某个语义的补齐常量：先看采纳布局的补齐清单，再看默认值表。
fn padding_value(plan: &ReconcilePlan, semantic: Semantic) -> Option<[f32; 4]> {
    plan.padding
        .iter()
        .find(|p| p.semantic == semantic)
        .map(|p| p.value)
        .or_else(|| semantic.default_value())
}

/// 按采纳布局交错打包：每顶点逐属性取值。
///
/// - 顺序 = `plan.layout.attribs` 的顺序（= shader location 顺序）；
/// - 某顶点缺该属性（数组短于位置数）时用补齐常量 —— 与补齐落表同源；
/// - 采纳布局里出现本函数不认识的语义 ⇒ 显式 panic（不静默写错字节）。
fn interleave(
    plan: &ReconcilePlan,
    positions: &[Vec3],
    uvs: &[Vec2],
    normals: Option<&[Vec3]>,
) -> Vec<f32> {
    let floats_per_vertex = plan.layout.stride as usize / 4;
    let mut out = Vec::with_capacity(positions.len() * floats_per_vertex);
    for (index, position) in positions.iter().enumerate() {
        for attrib in &plan.layout.attribs {
            let semantic = Semantic::of(attrib.semantic).unwrap_or_else(|| {
                panic!(
                    "不静默：3D 采纳布局里出现非契约语义 {:?}",
                    attrib.semantic
                )
            });
            let components = attrib.ty.size() as usize / 4;
            let value = match semantic {
                Semantic::Position => [position.x, position.y, position.z, 0.0],
                Semantic::Uv => uvs
                    .get(index)
                    .map(|uv| [uv.x, uv.y, 0.0, 0.0])
                    .or_else(|| padding_value(plan, semantic))
                    .expect("Uv 必须有默认值"),
                Semantic::ShadingNormal => normals
                    .and_then(|items| items.get(index))
                    .map(|normal| [normal.x, normal.y, normal.z, 0.0])
                    .or_else(|| padding_value(plan, semantic))
                    .expect("ShadingNormal 必须有默认值"),
                other => panic!("不静默：3D 采纳布局含暂无数据来源的语义 {other:?}"),
            };
            out.extend_from_slice(&value[..components]);
        }
    }
    out
}

/// 几何未携带 UV 时的兜底 UV（包围盒归一化到 [0, 1]）。
///
/// - [`UvProjection::Planar`]：XY 平面投影，**V 轴取反**（`v = 1 - y`）——
///   纹理 `v = 0` 是图像行首（顶），模型 `+y` 应显示图像上沿；否则贴图上下倒转
///   （与 `parametric_surface` 手写的 `1.0 - v` 同一口径）。
/// - [`UvProjection::Spherical`]：围绕包围盒中心的方向向量球面映射；
/// - [`UvProjection::Triplanar`]：按相对中心偏移最大的分量选主轴投影。
///
/// 退化包围盒（任一轴跨度为 0）按 1e-6 处理，避免除零。
///
/// 注意：本函数是**生成策略**（依赖包围盒与投影方式），不是默认值表里的标量常量，
/// 因此不并入 `Semantic::default_value`。
pub fn fallback_uvs(positions: &[Vec3], projection: UvProjection) -> Vec<Vec2> {
    if positions.is_empty() {
        return Vec::new();
    }
    let mut min = positions[0];
    let mut max = positions[0];
    for p in positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    let size = (max - min).max(Vec3::splat(1e-6));
    let center = (min + max) * 0.5;

    positions
        .iter()
        .map(|p| {
            let local = *p - min;
            match projection {
                UvProjection::Planar => Vec2::new(local.x / size.x, 1.0 - local.y / size.y),
                UvProjection::Spherical => {
                    let d = (*p - center).normalize_or_zero();
                    Vec2::new(d.x * 0.5 + 0.5, d.y * 0.5 + 0.5)
                }
                UvProjection::Triplanar => {
                    let rel = *p - center;
                    let (ax, ay) = if rel.x.abs() >= rel.y.abs() && rel.x.abs() >= rel.z.abs() {
                        (local.y / size.y, local.z / size.z)
                    } else if rel.y.abs() >= rel.z.abs() {
                        (local.x / size.x, local.z / size.z)
                    } else {
                        (local.x / size.x, local.y / size.y)
                    };
                    Vec2::new(ax, ay)
                }
            }
        })
        .collect()
}

/// 几何 → 顶点缓冲（UV 选择 + 调和补齐 + 交错打包，全部在生产端完成）。
///
/// 补齐不再走本函数里的 `unwrap_or`：缺槽由 [`reconcile`] 按
/// [`Semantic::default_value`] 的默认值表决定，本函数只负责按采纳布局写字节。
pub fn mesh_vertex_buffer(
    geometry: &MeshGeometry,
    projection: UvProjection,
) -> Result<VertexBufferData, VertexRecordError> {
    let plan = reconcile_3d(geometry.topology, geometry.normals.is_some());
    let uvs = match &geometry.uvs {
        Some(uvs) => uvs.clone(),
        None => fallback_uvs(&geometry.positions, projection),
    };
    let floats = interleave(
        &plan,
        &geometry.positions,
        &uvs,
        geometry.normals.as_deref(),
    );
    let indices = geometry.indices.clone().unwrap_or_default();
    Ok(VertexBufferData::new(
        plan.layout,
        bytes_of(&floats).to_vec(),
        indices,
    )?)
}

/// 3D 线段（两点）→ 顶点缓冲。
///
/// 线段只提供位置（无几何 UV / 法线）：缺的槽由调和按默认值表补齐 —— 与 mesh 路径
/// 同一张表、同一份机制，不再是本函数里的字面常量。line 无材质纹理 ⇒ UV 取什么
/// 都不参与着色；法线 `(0,0,1)` 配合默认头顶光得 `lit = 1`（颜色不受光照影响）。
pub fn line_vertex_buffer(start: Vec3, end: Vec3) -> Result<VertexBufferData, VertexRecordError> {
    let plan = reconcile_3d(PrimitiveTopology::Lines, false);
    let floats = interleave(&plan, &[start, end], &[], None);
    Ok(VertexBufferData::new(
        plan.layout,
        bytes_of(&floats).to_vec(),
        Vec::new(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_notion::Color32;
    use ze_scene_core::mesh_primitives::wireframe_cube;

    /// Planar 兜底 UV：图像行首（`v = 0`）对应模型 **+y**（矩形上沿），不得上下倒转。
    #[test]
    fn planar_fallback_uv_puts_image_top_at_positive_y() {
        let positions = vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        ];
        let uvs = fallback_uvs(&positions, UvProjection::Planar);
        assert_eq!(uvs[0], Vec2::new(0.0, 1.0), "左下角 → 图像左下");
        assert_eq!(uvs[1], Vec2::new(1.0, 1.0), "右下角 → 图像右下");
        assert_eq!(uvs[2], Vec2::new(0.0, 0.0), "左上角 → 图像左上");
        assert_eq!(uvs[3], Vec2::new(1.0, 0.0), "右上角 → 图像右上");
    }

    /// 3D 顶点缓冲：stride 32、顶点数正确、字节序与布局一致。
    #[test]
    fn mesh_vertex_buffer_matches_layout() {
        let geometry = wireframe_cube(1.0);
        let data = mesh_vertex_buffer(&geometry, UvProjection::Planar).unwrap();
        assert_eq!(data.layout.stride, 32);
        assert_eq!(data.vertex_count as usize, geometry.positions.len());
        data.validate().unwrap();
    }

    /// line 顶点缓冲：2 个顶点、无索引、法线 +Z。
    #[test]
    fn line_vertex_buffer_is_two_vertices() {
        let data = line_vertex_buffer(Vec3::ZERO, Vec3::X).unwrap();
        assert_eq!(data.vertex_count, 2);
        assert!(data.indices.is_empty());
        let words: Vec<f32> = data
            .vertices
            .chunks_exact(4)
            .map(|c| f32::from_ne_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(&words[5..8], &[0.0, 0.0, 1.0]);
        assert_eq!(&words[13..16], &[0.0, 0.0, 1.0]);
    }

    // ── 字节级判据：把手工兜底换成查表补齐后，输出必须**逐位不变** ──────────

    /// 顶点字节 → f32 序列（逐位比对用）。
    fn words_of(data: &VertexBufferData) -> Vec<f32> {
        data.vertices
            .chunks_exact(4)
            .map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap()))
            .collect()
    }

    /// mesh（无几何法线）：每顶点尾部补 `(0,0,1)`，字节序 `[pos, uv, normal]`。
    #[test]
    fn mesh_bytes_pad_missing_normal_with_plus_z() {
        let positions = vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, -2.0, -3.0)];
        let mut geometry =
            MeshGeometry::new(positions, Some(vec![0, 1]), PrimitiveTopology::Triangles);
        geometry.uvs = Some(vec![Vec2::new(0.25, 0.75), Vec2::new(0.5, 0.5)]);

        let data = mesh_vertex_buffer(&geometry, UvProjection::Planar).unwrap();
        assert_eq!(data.layout.stride, 32);
        assert_eq!(
            words_of(&data),
            vec![
                1.0, 2.0, 3.0, 0.25, 0.75, 0.0, 0.0, 1.0, //
                -1.0, -2.0, -3.0, 0.5, 0.5, 0.0, 0.0, 1.0,
            ]
        );
    }

    /// mesh（几何自带法线）：法线原样透传，不被补齐覆盖。
    #[test]
    fn mesh_bytes_keep_provided_normals() {
        let positions = vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let mut geometry =
            MeshGeometry::new(positions, Some(vec![0, 1]), PrimitiveTopology::Triangles);
        geometry.uvs = Some(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]);
        geometry.normals = Some(vec![Vec3::X, Vec3::Y]);

        let data = mesh_vertex_buffer(&geometry, UvProjection::Planar).unwrap();
        assert_eq!(
            words_of(&data),
            vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0,
            ]
        );
    }

    /// mesh（几何法线数组短于位置数）：缺的那几个顶点按默认值表补，已有的不动。
    #[test]
    fn mesh_bytes_pad_only_missing_normal_entries() {
        let positions = vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let mut geometry =
            MeshGeometry::new(positions, Some(vec![0, 1]), PrimitiveTopology::Triangles);
        geometry.uvs = Some(vec![Vec2::ZERO, Vec2::ZERO]);
        geometry.normals = Some(vec![Vec3::X]);

        let data = mesh_vertex_buffer(&geometry, UvProjection::Planar).unwrap();
        let words = words_of(&data);
        assert_eq!(&words[5..8], &[1.0, 0.0, 0.0], "第一个顶点的法线由几何提供");
        assert_eq!(&words[13..16], &[0.0, 0.0, 1.0], "第二个顶点补 (0,0,1)");
    }

    /// line 顶点缓冲的完整字节：位置 + `(0,0)` UV + `(0,0,1)` 法线。
    #[test]
    fn line_bytes_are_pinned() {
        let data = line_vertex_buffer(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)).unwrap();
        assert_eq!(data.layout.stride, 32, "本阶段 line 仍与 mesh 同布局");
        assert_eq!(
            words_of(&data),
            vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, //
                0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        );
    }

    /// 2D 生产：矩形填充 → stride 24、validate 通过（格式未变，D1）。
    #[test]
    fn tessellated_buffer_packs_2d_stride_24() {
        let shape = Shape2D::Rect(ze_notion::Rect2D::new(Vec2::ZERO, Vec2::new(10.0, 10.0)));
        let style = Style2D {
            fill: Some(Color32::WHITE),
            ..Style2D::default()
        };
        let buffers = tessellate_to_buffers(
            &shape,
            &Transform2D::identity(),
            &style,
            1.0,
        )
        .unwrap();
        assert!(!buffers.is_empty());
        for b in buffers {
            assert_eq!(b.data.layout.stride, 24);
            b.data.validate().unwrap();
        }
    }
}
