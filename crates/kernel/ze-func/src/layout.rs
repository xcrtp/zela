//! **Layout / Codec 的门面**：实现已迁至 `ze-type`（纯代数）与 `ze-reflect`（运行时注册表）。
//!
//! # 迁移后的分工
//!
//! | 内容 | 现在的位置 |
//! |---|---|
//! | `Layout` / `FieldLayout` / `FieldTy` / 编解码 | `ze-type::layout`（纯代数） |
//! | `TypeRegistry`（名字 → Layout） | `ze-reflect::codec`（运行时状态） |
//! | `ValueBytes` 包装 | 本模块（`ze-func` 认识 `ze-comb`，内核层不反向依赖） |
//!
//! # 端序显式固定
//!
//! 所有整数 / 浮点在字节层一律**小端**（[`ENDIAN`]）。不随目标机器变化。
//!
//! # 协商
//!
//! [`TypeRegistry::negotiate`] 是 Layout 协商的唯一入口：
//! - sender / receiver 布局**逐字段一致** ⇒ [`Transcode::ZeroCopy`]；
//! - 不一致或缺 Layout ⇒ [`Transcode::Reencoded`]（回退序列化）。
//!
//! 回退**可观测**：调用方拿得到 `Transcode`，性能问题不会变成谜。

use ze_comb::ValueBytes;

pub use ze_reflect::codec::{TypeRegistry, PATH_TYPE};
pub use ze_type::layout::{
    decode_path, decode_record, encode_path as encode_path_bytes, encode_record, ENDIAN, Endian,
    FieldLayout, FieldTy, Layout, LayoutError, Record, Scalar, Transcode, VEC2_FIELDS,
    VEC3_FIELDS,
};

/// `zela.Path` 的慢路径编码（返回命名类型 + 字节）。
///
/// `zela.Path` 是变长结构 ⇒ 没有 Layout、永远不走零拷贝。
pub fn encode_path(points: &[[f32; 2]], closed: bool) -> ValueBytes {
    ValueBytes::new(PATH_TYPE, encode_path_bytes(points, closed))
}

/// [`TypeRegistry::negotiate`] 的 `Value::Bytes` 版本：命名类型与字节一起搬运。
///
/// 这是"私有类型走同一机制"的入口 —— `zela.Path`（无 Layout）与
/// `acme.Tensor`（有 Layout）都从这里过。
pub fn negotiate_bytes(
    registry: &TypeRegistry,
    value: &ValueBytes,
    sender: Option<&Layout>,
) -> Result<(ValueBytes, Transcode), ze_type::ReflectError> {
    let (bytes, how) = registry.negotiate(&value.ty, &value.bytes, sender)?;
    Ok((ValueBytes::new(value.ty.clone(), bytes), how))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VEC2_LAYOUT: Layout = Layout::new(8, 4, VEC2_FIELDS);

    #[test]
    fn builtin_layouts_validate_and_match_their_sizes() {
        let registry = TypeRegistry::builtin();
        assert_eq!(registry.layout("zela.Vec2"), Some(VEC2_LAYOUT));
        assert_eq!(registry.layout("zela.Vec3").unwrap().size, 12);
        assert!(registry.layout("zela.Path").is_none(), "变长类型没有 Layout");
    }

    #[test]
    fn path_round_trips_on_the_slow_path() {
        let points = vec![[0.0, 1.0], [2.5, -3.0]];
        let bytes = encode_path(&points, true);
        assert_eq!(bytes.ty, PATH_TYPE);
        let (decoded, closed) = decode_path(&bytes.bytes).unwrap();
        assert_eq!(decoded, points);
        assert!(closed);
    }

    /// `Value::Bytes` 走同一协商入口：没有 Layout ⇒ 慢路径，原样搬运。
    #[test]
    fn value_bytes_use_the_same_negotiation_entry() {
        let registry = TypeRegistry::builtin();
        let path = encode_path(&[[0.0, 1.0]], false);
        let (same, how) = negotiate_bytes(&registry, &path, None).unwrap();
        assert_eq!(how, Transcode::Reencoded, "没有 Layout ⇒ 慢路径");
        assert_eq!(same, path);
    }

    #[test]
    fn third_party_types_use_the_same_registry() {
        let mut registry = TypeRegistry::builtin();
        let layout = Layout::custom(
            16,
            8,
            [("re", 0, FieldTy::F64), ("im", 8, FieldTy::F64)],
        )
        .unwrap();
        registry.register("acme.Tensor", Some(layout)).unwrap();
        assert_eq!(registry.layout("acme.Tensor"), Some(layout));

        let error = registry.register("acme.Tensor", None).unwrap_err();
        assert_eq!(error.name(), Some("acme.Tensor"));
        assert_eq!(error.class(), "重名");
    }
}
