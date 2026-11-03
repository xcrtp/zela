// reconcile.rs — 顶点语义的**反向调和**：shader 要什么 × mesh 给什么 → 采纳的布局 + 补齐计划。
//
// # 为什么需要
//
// wgpu（WebGPU 口径）**不支持常量顶点属性**：shader 声明了 `@location(n)`，管线顶点状态
// 就必须提供 location n，否则创建管线直接失败（`wgpu-core::validation::InputError::Missing`）。
// 因此"shader 需要但 mesh 没给"的槽，只能在**写出端**按默认值表补进顶点字节。
//
// 本模块只做决策，不碰字节：产出
// - 采纳的布局（顺序 = 需求顺序 = shader location 顺序）；
// - 补齐清单（哪个语义、什么类型、写什么常量）。
// 写出端（`zela-render::vertex_record`）按这份计划交错打包。
//
// # 位置
//
// 输入输出全是 ze-vertex 自己的类型，不碰 wgpu、不碰 IR ⇒ 纯函数、可全部单测。

use super::layout::{AttribType, VertexAttrib, VertexLayout};
use super::semantic::Semantic;

/// 「哪些槽要补、补什么」。
///
/// 无 `Eq`：`value` 是 f32（默认值表里的常量）。
#[derive(Debug, Clone, PartialEq)]
pub struct Padding {
    pub semantic: Semantic,
    /// 补齐所用的属性类型（[`Semantic::padding_type`]）。
    pub ty: AttribType,
    /// 补齐写入的常量（取前 `ty` 个分量）。
    pub value: [f32; 4],
}

/// 调和结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ReconcilePlan {
    /// 采纳的布局：属性顺序 = 需求顺序，stride 由 `compute_layout` 现算。
    ///
    /// `aa_model` / `cache` / `step_mode` 继承自提供方（缓存策略与步进模式是提供方的
    /// 事实，不因补齐而改变）。
    pub layout: VertexLayout,
    /// 补齐清单（按需求顺序）。空 = 提供方已经给了全部所需槽。
    pub padding: Vec<Padding>,
}

/// 调和失败。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReconcileError {
    #[error("需求语义 {0:?} 既未被提供，也没有默认值（不许猜值，请让提供方给出该槽）")]
    NoDefault(Semantic),
    #[error(
        "属性顺序冲突：需求要求 {required:?} 在前，但提供方把 {provided:?} 排在了它前面（不静默重排）"
    )]
    OrderMismatch {
        required: Semantic,
        provided: &'static str,
    },
}

/// 调和：按 `required` 的顺序产出采纳布局，缺的槽用默认值表补齐。
///
/// 规则：
/// 1. **顺序保持**：`provided` 中属于需求集合的属性，其相对顺序必须与 `required` 一致；
///    否则报 [`ReconcileError::OrderMismatch`]（不静默重排 —— 重排会让同一份语义产出
///    不同字节序）。
/// 2. **按语义取属性**：需求里每个语义，优先用提供方的属性（沿用其类型与语义名）。
/// 3. **缺失即补齐**：提供方没有该语义时查默认值表；表里也没有（只有 `Position`）
///    则报 [`ReconcileError::NoDefault`]。
/// 4. 提供方有、需求不要的属性**被丢弃**（这是"缩窄布局"的机制）。
pub fn reconcile(
    required: &[Semantic],
    provided: &VertexLayout,
) -> Result<ReconcilePlan, ReconcileError> {
    // ① 顺序保持校验：provided 里落在需求集合内的属性，其需求下标必须严格递增。
    let mut last: Option<(usize, &'static str)> = None;
    for attrib in &provided.attribs {
        let Some(semantic) = Semantic::of(attrib.semantic) else {
            // 宿主自定义语义（实例数据等）不参与顶点需求，忽略。
            continue;
        };
        let Some(position) = required.iter().position(|r| *r == semantic) else {
            // 需求不要这个槽 —— 会被丢弃，不参与顺序校验。
            continue;
        };
        if let Some((previous, previous_name)) = last {
            if position <= previous {
                return Err(ReconcileError::OrderMismatch {
                    required: required[position],
                    provided: previous_name,
                });
            }
        }
        last = Some((position, attrib.semantic));
    }

    // ② 逐需求生成属性；缺的槽记一条补齐。
    let mut attribs: Vec<VertexAttrib> = Vec::with_capacity(required.len());
    let mut padding: Vec<Padding> = Vec::new();
    for semantic in required {
        let provided_attrib = provided
            .attribs
            .iter()
            .find(|a| Semantic::of(a.semantic) == Some(*semantic));
        match provided_attrib {
            Some(attrib) => attribs.push(VertexAttrib {
                semantic: attrib.semantic,
                ty: attrib.ty,
                offset: 0,
            }),
            None => {
                let value = semantic
                    .default_value()
                    .ok_or(ReconcileError::NoDefault(*semantic))?;
                let ty = semantic.padding_type();
                padding.push(Padding {
                    semantic: *semantic,
                    ty,
                    value,
                });
                attribs.push(VertexAttrib {
                    semantic: semantic.as_str(),
                    ty,
                    offset: 0,
                });
            }
        }
    }

    let stride = VertexLayout::compute_layout(&mut attribs);
    Ok(ReconcilePlan {
        layout: VertexLayout {
            attribs,
            stride,
            // 缓存策略 / 步进模式是提供方的事实，补齐不改变它们。
            aa_model: provided.aa_model,
            cache: provided.cache,
            step_mode: provided.step_mode,
        },
        padding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aa::AaModel;
    use crate::cache::CacheStrategy;

    fn layout_of(semantics: &[Semantic]) -> VertexLayout {
        let mut attribs: Vec<VertexAttrib> = semantics
            .iter()
            .map(|s| VertexAttrib {
                semantic: s.as_str(),
                ty: s.padding_type(),
                offset: 0,
            })
            .collect();
        let stride = VertexLayout::compute_layout(&mut attribs);
        VertexLayout {
            attribs,
            stride,
            aa_model: AaModel::None,
            cache: CacheStrategy::Static,
            step_mode: crate::layout::StepMode::Vertex,
        }
    }

    const FULL_3D: [Semantic; 3] = [
        Semantic::Position,
        Semantic::Uv,
        Semantic::ShadingNormal,
    ];

    /// 需求 ⊆ 提供 ⇒ 无补齐，布局与提供方逐项一致（stride 32）。
    #[test]
    fn fully_provided_needs_no_padding() {
        let provided = layout_of(&FULL_3D);
        let plan = reconcile(&FULL_3D, &provided).unwrap();
        assert!(plan.padding.is_empty(), "全部槽都有 ⇒ 不应有补齐");
        assert_eq!(plan.layout.stride, provided.stride);
        assert_eq!(plan.layout.attribs, provided.attribs);
    }

    /// 缺 UV ⇒ 一条补齐，值是 (0,0)，宽度 2；stride 仍是 32。
    #[test]
    fn missing_uv_is_padded_with_zero() {
        let provided = layout_of(&[Semantic::Position, Semantic::ShadingNormal]);
        let plan = reconcile(&FULL_3D, &provided).unwrap();
        assert_eq!(plan.padding.len(), 1);
        assert_eq!(plan.padding[0].semantic, Semantic::Uv);
        assert_eq!(plan.padding[0].ty, AttribType::F32x2);
        assert_eq!(plan.padding[0].value, [0.0, 0.0, 0.0, 0.0]);
        // 顺序 = 需求顺序 ⇒ P@0(12) + U@12(8) + N@20(12) = 32
        assert_eq!(plan.layout.attribs[1].semantic, "uv");
        assert_eq!(plan.layout.attribs[1].offset, 12);
        assert_eq!(plan.layout.stride, 32);
    }

    /// 缺法线 ⇒ 一条补齐，值是 (0,0,1)（与默认光点积为 1 ⇒ lit = 1）。
    #[test]
    fn missing_normal_is_padded_with_plus_z() {
        let provided = layout_of(&[Semantic::Position, Semantic::Uv]);
        let plan = reconcile(&FULL_3D, &provided).unwrap();
        assert_eq!(plan.padding.len(), 1);
        assert_eq!(plan.padding[0].semantic, Semantic::ShadingNormal);
        assert_eq!(plan.padding[0].ty, AttribType::F32x3);
        assert_eq!(plan.padding[0].value, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(plan.layout.stride, 32);
        assert_eq!(plan.layout.attribs[2].offset, 20);
    }

    /// 缺位置 ⇒ 显式报错（位置没有默认值，不许猜）。
    #[test]
    fn missing_position_is_explicit_error() {
        let provided = layout_of(&[Semantic::Uv, Semantic::ShadingNormal]);
        let error = reconcile(&FULL_3D, &provided).unwrap_err();
        assert_eq!(error, ReconcileError::NoDefault(Semantic::Position));
    }

    /// 顺序冲突 ⇒ 显式报错（不静默重排）。
    ///
    /// 提供方把 `uv` 排在 `position` 前面，但需求要求 `position` 在前。
    #[test]
    fn reversed_order_is_explicit_error() {
        let provided = layout_of(&[Semantic::Uv, Semantic::Position]);
        let error = reconcile(&FULL_3D, &provided).unwrap_err();
        assert_eq!(
            error,
            ReconcileError::OrderMismatch {
                required: Semantic::Position,
                provided: "uv",
            }
        );
    }

    /// 提供方多给的槽被丢弃（缩窄布局的机制）：需求只要位置 ⇒ stride 12。
    #[test]
    fn extra_provided_slots_are_dropped() {
        let provided = layout_of(&FULL_3D);
        let plan = reconcile(&[Semantic::Position], &provided).unwrap();
        assert!(plan.padding.is_empty());
        assert_eq!(plan.layout.attribs.len(), 1);
        assert_eq!(plan.layout.attribs[0].semantic, "position");
        assert_eq!(plan.layout.stride, 12);
    }

    /// 宿主自定义语义（实例化 buffer）不参与调和：既不报错也不进采纳布局。
    #[test]
    fn custom_semantics_are_ignored() {
        let mut provided = layout_of(&FULL_3D);
        provided.attribs.push(VertexAttrib {
            semantic: "instance_offset",
            ty: AttribType::F32x4,
            offset: 0,
        });
        let plan = reconcile(&FULL_3D, &provided).unwrap();
        assert_eq!(plan.layout.attribs.len(), 3);
        assert!(
            plan.layout
                .attribs
                .iter()
                .all(|a| a.semantic != "instance_offset")
        );
    }

    /// 采纳布局必须自洽（offset 不重叠、stride 覆盖全部属性）。
    #[test]
    fn reconciled_layout_validates() {
        for provided in [
            layout_of(&FULL_3D),
            layout_of(&[Semantic::Position]),
            layout_of(&[Semantic::Position, Semantic::Uv]),
        ] {
            let plan = reconcile(&FULL_3D, &provided).unwrap();
            plan.layout.validate().unwrap();
        }
    }
}
