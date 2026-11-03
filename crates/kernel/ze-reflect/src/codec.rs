//! **Codec**：跨边界 `Bytes ↔ typed`，慢路径的正式名分。
//!
//! # 为什么慢路径要有名字
//!
//! 零拷贝是协商成功的**奖励**，不是唯一路径。删掉序列化路径 ⇒ 跨 wasm 立刻死。
//! 把慢路径命名成 [`Codec`]，它就不再是"失败时的兜底"，而是与零拷贝并列的一条
//! 正式路径。
//!
//! # 注册表是运行时状态
//!
//! `TypeRegistry`（名字 → Layout）持有 `BTreeMap`，因此属于本 crate 而不是
//! `ze-type`（硬约束 6：ze-type 零运行时状态）。

use std::collections::BTreeMap;

use ze_type::layout::{
    decode_path, encode_path, decode_record, encode_record,
};
use ze_type::{Blame, Layout, LayoutError, NameKind, Record, ReflectError, Transcode};

/// 命名类型的注册表：名字 → Layout（`None` = 没有 Layout，永远走慢路径）。
#[derive(Clone, Debug, Default)]
pub struct TypeRegistry {
    entries: BTreeMap<String, Option<Layout>>,
}

impl TypeRegistry {
    /// 空表（第三方从零注册时用）。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 内建类型：`zela.F64` / `zela.I64` / `zela.Bool` / `zela.Vec2` / `zela.Vec3` / `zela.Path`。
    ///
    /// `zela.Path` **没有** Layout（变长结构）⇒ 安全缺省：慢路径。
    pub fn builtin() -> Self {
        use ze_type::layout::{VEC2_FIELDS, VEC3_FIELDS};
        let mut registry = Self::empty();
        registry
            .register("zela.F64", Some(Layout::new(8, 8, &[])))
            .expect("内建布局固定且自检通过");
        registry
            .register("zela.I64", Some(Layout::new(8, 8, &[])))
            .expect("内建布局固定且自检通过");
        registry
            .register("zela.Bool", Some(Layout::new(1, 1, &[])))
            .expect("内建布局固定且自检通过");
        registry
            .register("zela.Vec2", Some(Layout::new(8, 4, VEC2_FIELDS)))
            .expect("内建布局固定且自检通过");
        registry
            .register("zela.Vec3", Some(Layout::new(12, 4, VEC3_FIELDS)))
            .expect("内建布局固定且自检通过");
        registry
            .register("zela.Path", None)
            .expect("内建类型名唯一");
        registry
    }

    /// 注册一个类型（`layout = None` ⇒ 慢路径）。
    ///
    /// 重复注册是**显式冲突**：类型名是跨边界的身份，静默覆盖会让两端各说各话。
    /// 这是"一个诊断类型"的实例 —— 与 `FunctionId` / 表达式 id 的重名同源。
    pub fn register(
        &mut self,
        name: impl Into<String>,
        layout: Option<Layout>,
    ) -> Result<(), ReflectError> {
        let name = name.into();
        if self.entries.contains_key(&name) {
            return Err(ReflectError::duplicate(
                NameKind::Type,
                name,
                Blame::Assembly,
            ));
        }
        if let Some(layout) = &layout {
            layout.validate()?;
        }
        self.entries.insert(name, layout);
        Ok(())
    }

    /// 查布局；类型未注册 ⇒ `None`（"没有 Layout"是安全缺省）。
    pub fn layout(&self, ty: &str) -> Option<Layout> {
        self.entries.get(ty).copied().flatten()
    }

    pub fn contains(&self, ty: &str) -> bool {
        self.entries.contains_key(ty)
    }

    /// 已注册的 `(名字, 是否有 Layout)` 清单（文档 / 对拍用）。
    pub fn entries(&self) -> impl Iterator<Item = (&str, bool)> {
        self.entries
            .iter()
            .map(|(name, layout)| (name.as_str(), layout.is_some()))
    }

    /// **Layout 协商**：同布局 ⇒ 零拷贝；否则解码后按目标布局重编码。
    ///
    /// `sender` 是发送端的布局（`None` = 发送端没有 Layout，按回退格式解码）。
    pub fn negotiate(
        &self,
        ty: &str,
        bytes: &[u8],
        sender: Option<&Layout>,
    ) -> Result<(Vec<u8>, Transcode), ReflectError> {
        let receiver = self.layout(ty);
        match (sender, receiver) {
            (Some(sender), Some(receiver)) if sender == &receiver => {
                Ok((bytes.to_vec(), Transcode::ZeroCopy))
            }
            // 两端都没有 Layout ⇒ 无法（也不需要）协商：按命名类型自己的慢路径原样搬运。
            (None, None) => Ok((bytes.to_vec(), Transcode::Reencoded)),
            _ => {
                let record = decode_record(ty, bytes, sender.copied())?;
                let reencoded = encode_record(&record, receiver)?;
                Ok((reencoded, Transcode::Reencoded))
            }
        }
    }
}

/// 慢路径的正式协议：命名类型 + 字节 ↔ typed 值。
pub trait Codec {
    /// typed 形式。
    type Item;

    fn decode(&self, ty: &str, bytes: &[u8], layout: Option<Layout>) -> Result<Self::Item, LayoutError>;

    fn encode(&self, item: &Self::Item, layout: Option<Layout>) -> Result<Vec<u8>, LayoutError>;
}

/// `Record` 的 Codec（无 Layout ⇒ 回退格式）。
#[derive(Clone, Copy, Debug, Default)]
pub struct RecordCodec;

impl Codec for RecordCodec {
    type Item = Record;

    fn decode(&self, ty: &str, bytes: &[u8], layout: Option<Layout>) -> Result<Record, LayoutError> {
        decode_record(ty, bytes, layout)
    }

    fn encode(&self, item: &Record, layout: Option<Layout>) -> Result<Vec<u8>, LayoutError> {
        encode_record(item, layout)
    }
}

/// `zela.Path` 的 Codec（变长 ⇒ 永远慢路径）。
#[derive(Clone, Copy, Debug, Default)]
pub struct PathCodec;

impl Codec for PathCodec {
    type Item = (Vec<[f32; 2]>, bool);

    fn decode(
        &self,
        _ty: &str,
        bytes: &[u8],
        _layout: Option<Layout>,
    ) -> Result<Self::Item, LayoutError> {
        decode_path(bytes)
    }

    fn encode(&self, item: &Self::Item, _layout: Option<Layout>) -> Result<Vec<u8>, LayoutError> {
        Ok(encode_path(&item.0, item.1))
    }
}

/// `zela.Path` 的注册名。
pub const PATH_TYPE: &str = ze_type::layout::PATH_TYPE;

/// `zela.Path` 的慢路径编解码（字节层；`ValueBytes` 包装留在 `ze-func`）。
pub fn encode_path_bytes(points: &[[f32; 2]], closed: bool) -> Vec<u8> {
    encode_path(points, closed)
}

pub fn decode_path_bytes(bytes: &[u8]) -> Result<(Vec<[f32; 2]>, bool), LayoutError> {
    decode_path(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_type::layout::VEC2_FIELDS;
    use ze_type::Record;
    use ze_type::Scalar;

    const VEC2_LAYOUT: Layout = Layout::new(8, 4, VEC2_FIELDS);
    const SWAPPED_FIELDS: &[ze_type::FieldLayout] = &[
        ze_type::FieldLayout::new("y", 0, ze_type::FieldTy::F32),
        ze_type::FieldLayout::new("x", 4, ze_type::FieldTy::F32),
    ];
    const SWAPPED_LAYOUT: Layout = Layout::new(8, 4, SWAPPED_FIELDS);

    #[test]
    fn builtin_layouts_validate_and_path_has_none() {
        let registry = TypeRegistry::builtin();
        assert_eq!(registry.layout("zela.Vec2"), Some(VEC2_LAYOUT));
        assert_eq!(registry.layout("zela.Vec3").unwrap().size, 12);
        assert!(registry.layout("zela.Path").is_none(), "变长类型没有 Layout");
    }

    #[test]
    fn zero_copy_when_layouts_match_and_reencode_when_they_differ() {
        let registry = TypeRegistry::builtin();
        let record = Record::new(
            "zela.Vec2",
            vec![
                ("x".to_string(), Scalar::F32(1.0)),
                ("y".to_string(), Scalar::F32(2.0)),
            ],
        );
        let bytes = encode_record(&record, Some(VEC2_LAYOUT)).unwrap();
        let (same, how) = registry
            .negotiate("zela.Vec2", &bytes, Some(&VEC2_LAYOUT))
            .unwrap();
        assert_eq!(how, Transcode::ZeroCopy);
        assert_eq!(same, bytes, "零拷贝必须逐字节相同");

        let from_swapped = encode_record(&record, Some(SWAPPED_LAYOUT)).unwrap();
        let (reencoded, how) = registry
            .negotiate("zela.Vec2", &from_swapped, Some(&SWAPPED_LAYOUT))
            .unwrap();
        assert_eq!(how, Transcode::Reencoded);
        let decoded = decode_record("zela.Vec2", &reencoded, registry.layout("zela.Vec2")).unwrap();
        assert_eq!(decoded, record, "布局不一致 ⇒ 回退且结果一致");
    }

    #[test]
    fn third_party_types_use_the_same_registry_and_duplicates_are_conflicts() {
        let mut registry = TypeRegistry::builtin();
        let layout = Layout::custom(
            16,
            8,
            [
                ("re", 0, ze_type::FieldTy::F64),
                ("im", 8, ze_type::FieldTy::F64),
            ],
        )
        .unwrap();
        registry.register("acme.Tensor", Some(layout)).unwrap();
        assert_eq!(registry.layout("acme.Tensor"), Some(layout));

        let error = registry.register("acme.Tensor", None).unwrap_err();
        assert_eq!(error.name(), Some("acme.Tensor"));
        assert_eq!(error.class(), "重名");
    }

    #[test]
    fn slow_path_is_a_named_codec_not_a_fallback() {
        let points = vec![[0.0, 1.0], [2.5, -3.0]];
        let bytes = PathCodec.encode(&(points.clone(), true), None).unwrap();
        let decoded = PathCodec.decode(PATH_TYPE, &bytes, None).unwrap();
        assert_eq!(decoded, (points, true));
    }
}
