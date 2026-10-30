// semantic.rs — 顶点属性的**类型化语义**与默认值表。
//
// # 定位
// `VertexAttrib.semantic` 是 `&'static str`（参与 serde，且实例化 buffer 允许宿主自定义
// 语义），本文件给出一份**枚举视图**：编译期检查、需求声明、默认值表、调和规则都用它。
// 字段类型不变 —— 形态是「字符串字段 + 枚举视图」，避免把宿主自定义语义挤进枚举。
//
// # 与 2D `AttribSemantic` 的关系（**并存，不合并**）
// - [`AttribSemantic`](super::aa::AttribSemantic) 是「某个槽在某个 `AaModel` 下扮演什么角色」，
//   模型内相对语义：`AaModel::None` 下三个槽全是 `Undefined`；
// - [`Semantic`] 是「这个属性本身是全局面上的什么」，绝对语义。
//
// 特别注意：`AttribSemantic::Normal` 是 **2D 边法线**，不是 [`Semantic::ShadingNormal`]
// （3D 着色法线）。两者层次不同，不得互相替代 —— 这也是 3D 法线命名为 `shading_normal`
// 而不是 `normal` 的原因。
//
// # 默认值表（正向调和）
// 语义缺失时写进顶点字节的常量。取值是**既有行为**（`zela-render` 写出端的手工兜底），
// 逐位沿用，不得顺手优化：
// - `Uv → (0,0)`：无纹理语义的图元不消费 UV，缺纹理时绑 1×1 透明黑，采样值恒 0；
// - `ShadingNormal → (0,0,1)`：与默认头顶光 `L=(0,0,1)` 点积为 1 ⇒ `lit = 1` ⇒
//   外观与加光照之前逐位一致（见 `cube.wgsl` 顶部说明）。换值会让线框 / 平面片像素动。

/// 顶点属性的绝对语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Semantic {
    /// 位置。**没有默认值** —— 位置无法臆造，缺失即显式报错。
    Position,
    /// 纹理坐标。
    Uv,
    /// 3D 着色法线（与 2D 的边法线 `AttribSemantic::Normal` 不是同一件事）。
    ShadingNormal,
    /// 2D 通用槽 0（含义由 `AaModel` 解释）。
    Attrib0,
    /// 2D 通用槽 1。
    Attrib1,
    /// 2D 通用槽 2。
    Attrib2,
    /// 骨骼索引（预留：本批只声明语义，不含动画系统）。
    Joints0,
    /// 骨骼权重（预留）。
    Weights0,
}

impl Semantic {
    /// 语义个数（= 契约词汇表的长度）。供 `VertexAttrib::VOCABULARY` 的数组长度使用。
    pub const COUNT: usize = Self::ALL.len();

    /// 全部语义（顺序 = 契约词汇表的顺序，见 [`super::VertexAttrib::VOCABULARY`]）。
    pub const ALL: [Semantic; 8] = [
        Self::Position,
        Self::Uv,
        Self::ShadingNormal,
        Self::Attrib0,
        Self::Attrib1,
        Self::Attrib2,
        Self::Joints0,
        Self::Weights0,
    ];

    /// 契约词汇表里的字符串名（与 `VertexAttrib.semantic` 逐字一致）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Uv => "uv",
            Self::ShadingNormal => "shading_normal",
            Self::Attrib0 => "attrib0",
            Self::Attrib1 => "attrib1",
            Self::Attrib2 => "attrib2",
            Self::Joints0 => "joints0",
            Self::Weights0 => "weights0",
        }
    }

    /// 字符串名 → 语义。非词汇表的名字（宿主自定义）返回 `None`。
    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.as_str() == name)
    }

    /// 默认值表：该语义缺失时写进顶点字节的常量。
    ///
    /// `None` = 该语义**没有默认值**，缺失必须显式报错（不猜值）。
    pub const fn default_value(self) -> Option<[f32; 4]> {
        match self {
            // 位置无法臆造。
            Self::Position => None,
            // 兜底 UV：与 `zela-render` 写出端的 `Vec2::ZERO` 逐位一致。
            Self::Uv => Some([0.0, 0.0, 0.0, 0.0]),
            // 兜底法线：与默认光 `(0,0,1)` 点积为 1 ⇒ lit = 1（像素与加光照前一致）。
            Self::ShadingNormal => Some([0.0, 0.0, 1.0, 0.0]),
            // 2D 通用槽：三角化器必然产齐，此值只用于跨模型误用时的显式补齐。
            Self::Attrib0 | Self::Attrib1 | Self::Attrib2 => Some([0.0, 0.0, 0.0, 0.0]),
            // 骨骼索引默认全 0（全部指向骨骼 0）。
            Self::Joints0 => Some([0.0, 0.0, 0.0, 0.0]),
            // 骨骼权重默认全 0 = **无骨骼影响**。不能用 `[1,0,0,0]`：那会把所有顶点
            // 绑到骨骼 0，且与「未提供骨骼数据」的语义相反。
            Self::Weights0 => Some([0.0, 0.0, 0.0, 0.0]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 字符串名与枚举一一往返（`as_str` ↔ `of`）。
    #[test]
    fn as_str_of_roundtrip() {
        for s in Semantic::ALL {
            assert_eq!(Semantic::of(s.as_str()), Some(s), "{s:?} 往返失败");
        }
        // 宿主自定义语义（实例化 buffer）不在词汇表内。
        assert_eq!(Semantic::of("instance_offset"), None);
        assert_eq!(Semantic::of(""), None);
    }

    /// 字符串名互不重复，且全部非空。
    #[test]
    fn names_are_unique_and_non_empty() {
        let names: Vec<&str> = Semantic::ALL.iter().map(|s| s.as_str()).collect();
        let unique: HashSet<&&str> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "语义名必须互不重复：{names:?}");
        assert!(names.iter().all(|n| !n.is_empty()));
    }

    /// 默认值表逐项固定（改值即像素风险，本用例是那道闸）。
    #[test]
    fn default_values_are_pinned() {
        assert_eq!(Semantic::Position.default_value(), None, "位置无默认值");
        assert_eq!(
            Semantic::Uv.default_value(),
            Some([0.0, 0.0, 0.0, 0.0]),
            "兜底 UV 必须是 (0,0)"
        );
        assert_eq!(
            Semantic::ShadingNormal.default_value(),
            Some([0.0, 0.0, 1.0, 0.0]),
            "兜底法线必须是 (0,0,1)：与默认光点积为 1 ⇒ lit = 1"
        );
        for s in [Semantic::Attrib0, Semantic::Attrib1, Semantic::Attrib2] {
            assert_eq!(s.default_value(), Some([0.0; 4]), "{s:?} 默认值应为全零");
        }
        assert_eq!(Semantic::Joints0.default_value(), Some([0.0; 4]));
        assert_eq!(
            Semantic::Weights0.default_value(),
            Some([0.0; 4]),
            "权重兜底必须全零 = 无骨骼影响（不能是 [1,0,0,0]）"
        );
    }

    /// 只有 `Position` 没有默认值 —— 保证「缺位置必须报错」这条不会被后来者放宽。
    #[test]
    fn only_position_has_no_default() {
        let without: Vec<Semantic> = Semantic::ALL
            .iter()
            .copied()
            .filter(|s| s.default_value().is_none())
            .collect();
        assert_eq!(without, vec![Semantic::Position]);
    }
}
