//! 类型注册 + **Layout**：让离散层状态与跨边界的大块数据有一份**显式**描述。
//!
//! # 为什么 Layout 只对 `#[repr(C)]` 生成
//!
//! 没有 Layout 的合法后果是"走序列化慢路径"，而不是猜一个布局。硬约束 4：
//! **「没 Layout」是安全缺省** —— `zela.Path` 是变长结构，因此注册为 `None`，
//! 永远走慢路径；`zela.Vec2` / `zela.Vec3` 是定长 `#[repr(C)]`，注册为 `Some`。
//!
//! # 端序显式固定
//!
//! 所有整数 / 浮点在字节层一律**小端**（[`ENDIAN`]）。不随目标机器变化，
//! 避免"多数机器对、少数机器错"。
//!
//! # 协商
//!
//! [`TypeRegistry::negotiate`] 是 Layout 协商的唯一入口：
//! - sender / receiver 布局**逐字段一致** ⇒ [`Transcode::ZeroCopy`]（原字节直接搬运）；
//! - 不一致或缺 Layout ⇒ [`Transcode::Reencoded`]（回退序列化）。
//!
//! 回退**可观测**：调用方拿得到 `Transcode`，性能问题不会变成谜。

use std::collections::BTreeMap;

use serde::Serialize;
use ze_comb::ValueBytes;

/// 字节层端序（进产物：跨平台一致性的一部分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Endian {
    Little,
    Big,
}

/// 端序**固定为小端**。
pub const ENDIAN: Endian = Endian::Little;

/// `zela.Vec2` 的字段表（`'static`，进布局常量）。
pub const VEC2_FIELDS: &[FieldLayout] = &[
    FieldLayout::new("x", 0, FieldTy::F32),
    FieldLayout::new("y", 4, FieldTy::F32),
];

/// `zela.Vec3` 的字段表。
pub const VEC3_FIELDS: &[FieldLayout] = &[
    FieldLayout::new("x", 0, FieldTy::F32),
    FieldLayout::new("y", 4, FieldTy::F32),
    FieldLayout::new("z", 8, FieldTy::F32),
];

/// 字段的标量类型（决定宽度；**不靠名字猜**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldTy {
    F32,
    F64,
    I64,
    Bool,
}

impl FieldTy {
    pub const fn width(self) -> usize {
        match self {
            FieldTy::F32 => 4,
            FieldTy::F64 | FieldTy::I64 => 8,
            FieldTy::Bool => 1,
        }
    }
}

/// 字段布局：名字 + 字节偏移 + 标量类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FieldLayout {
    pub name: &'static str,
    pub offset: usize,
    pub ty: FieldTy,
}

impl FieldLayout {
    pub const fn new(name: &'static str, offset: usize, ty: FieldTy) -> Self {
        Self { name, offset, ty }
    }
}

/// 定长类型布局：`size` / `align` / 字段偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Layout {
    pub size: usize,
    pub align: usize,
    pub fields: &'static [FieldLayout],
}

impl Layout {
    pub const fn new(size: usize, align: usize, fields: &'static [FieldLayout]) -> Self {
        Self { size, align, fields }
    }

    /// 自定义布局（第三方类型注册时用），在**运行时**校验。
    pub fn custom(
        size: usize,
        align: usize,
        fields: impl IntoIterator<Item = (impl Into<String>, usize, FieldTy)>,
    ) -> Result<Self, LayoutError> {
        let fields: Vec<(String, usize, FieldTy)> = fields
            .into_iter()
            .map(|(name, offset, ty)| (name.into(), offset, ty))
            .collect();
        // 名字必须是 `'static` 才能进入本结构；第三方用 leak 一份名字（注册是低频操作）。
        let leaked: &'static [FieldLayout] = Box::leak(
            fields
                .iter()
                .map(|(name, offset, ty)| FieldLayout {
                    name: Box::leak(name.clone().into_boxed_str()),
                    offset: *offset,
                    ty: *ty,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let layout = Self::new(size, align, leaked);
        layout.validate()?;
        Ok(layout)
    }

    /// 布局自检：对齐是 2 的幂、字段不越界、互不重叠。
    pub fn validate(&self) -> Result<(), LayoutError> {
        if self.size == 0 {
            return Err(LayoutError::ZeroSize);
        }
        if self.align == 0 || !self.align.is_power_of_two() {
            return Err(LayoutError::BadAlign { align: self.align });
        }
        let mut spans: Vec<(usize, usize, &'static str)> = Vec::new();
        for field in self.fields {
            let width = field.ty.width();
            let Some(end) = field.offset.checked_add(width) else {
                return Err(LayoutError::FieldOutOfBounds {
                    field: field.name,
                    offset: field.offset,
                    size: self.size,
                });
            };
            if end > self.size {
                return Err(LayoutError::FieldOutOfBounds {
                    field: field.name,
                    offset: field.offset,
                    size: self.size,
                });
            }
            spans.push((field.offset, end, field.name));
        }
        spans.sort_by_key(|(start, _, _)| *start);
        for pair in spans.windows(2) {
            let (_, left_end, left_name) = pair[0];
            let (right_start, _, right_name) = pair[1];
            if right_start < left_end {
                return Err(LayoutError::Overlap {
                    left: left_name,
                    right: right_name,
                    at: right_start,
                });
            }
        }
        Ok(())
    }

    /// 按名字查字段偏移。
    pub fn offset_of(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.offset)
    }
}

/// 布局错误（全部显式）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    #[error("布局的 size 不能为 0")]
    ZeroSize,
    #[error("布局的 align 必须是 2 的幂，实际 {align}")]
    BadAlign { align: usize },
    #[error("字段 `{field}`（offset {offset}）越过布局边界 size = {size}")]
    FieldOutOfBounds {
        field: &'static str,
        offset: usize,
        size: usize,
    },
    #[error("字段 `{left}` 与 `{right}` 在 offset {at} 处重叠")]
    Overlap {
        left: &'static str,
        right: &'static str,
        at: usize,
    },
    #[error("类型 `{0}` 未注册")]
    UnknownType(String),
    #[error("类型 `{0}` 重复注册")]
    DuplicateType(String),
    #[error("记录字段 `{field}` 不存在于类型 `{ty}` 的布局里")]
    UnknownField { ty: String, field: String },
    #[error("类型 `{ty}` 缺少字段 `{field}`")]
    MissingField { ty: String, field: String },
    #[error("字节长度 {len} 与类型 `{ty}` 的 size {size} 不符")]
    BadLength { ty: String, len: usize, size: usize },
    #[error("记录里出现不可编码的标量：{0}")]
    UnsupportedScalar(&'static str),
    #[error("回退编码损坏：{0}")]
    CorruptFallback(String),
}

/// 标量（布局层的最小值集合；与表达式 `Value` 一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Scalar {
    F32(f32),
    F64(f64),
    I64(i64),
    Bool(bool),
}

/// 一条按名取值的记录（Layout 协商的输入 / 输出）。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Record {
    pub ty: String,
    pub fields: Vec<(String, Scalar)>,
}

impl Record {
    pub fn new(ty: impl Into<String>, fields: Vec<(String, Scalar)>) -> Self {
        Self {
            ty: ty.into(),
            fields,
        }
    }

    pub fn get(&self, name: &str) -> Option<Scalar> {
        self.fields
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| *value)
    }
}

/// 布局协商的结果（**回退可观测**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transcode {
    /// 两端布局逐字段一致 ⇒ 原字节直接搬运。
    ZeroCopy,
    /// 布局不一致或缺 Layout ⇒ 回退序列化（慢路径）。
    Reencoded,
}

/// 类型注册表：名字 → Layout（`None` = 没有 Layout，永远走慢路径）。
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
            .register(
                "zela.Vec2",
                Some(Layout::new(8, 4, VEC2_FIELDS)),
            )
            .expect("内建布局固定且自检通过");
        registry
            .register(
                "zela.Vec3",
                Some(Layout::new(12, 4, VEC3_FIELDS)),
            )
            .expect("内建布局固定且自检通过");
        registry
            .register("zela.Path", None)
            .expect("内建类型名唯一");
        registry
    }

    /// 注册一个类型（`layout = None` ⇒ 慢路径）。
    ///
    /// 重复注册是**显式错误**：类型名是跨边界的身份，静默覆盖会让两端各说各话。
    pub fn register(
        &mut self,
        name: impl Into<String>,
        layout: Option<Layout>,
    ) -> Result<(), LayoutError> {
        let name = name.into();
        if self.entries.contains_key(&name) {
            return Err(LayoutError::DuplicateType(name));
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

    /// 类型是否已注册。
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
    ) -> Result<(Vec<u8>, Transcode), LayoutError> {
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

    /// [`TypeRegistry::negotiate`] 的 `Value::Bytes` 版本：命名类型与字节一起搬运。
    ///
    /// 这是"私有类型走同一机制"的入口 —— `zela.Path`（无 Layout）与
    /// `acme.Tensor`（有 Layout）都从这里过。
    pub fn negotiate_bytes(
        &self,
        value: &ValueBytes,
        sender: Option<&Layout>,
    ) -> Result<(ValueBytes, Transcode), LayoutError> {
        let (bytes, how) = self.negotiate(&value.ty, &value.bytes, sender)?;
        Ok((ValueBytes::new(value.ty.clone(), bytes), how))
    }
}

/// 按布局编码一条记录（`layout = None` ⇒ 回退格式）。
pub fn encode_record(record: &Record, layout: Option<Layout>) -> Result<Vec<u8>, LayoutError> {
    match layout {
        Some(layout) => {
            layout.validate()?;
            let mut out = vec![0u8; layout.size];
            for field in layout.fields {
                let value = record.get(field.name).ok_or_else(|| LayoutError::MissingField {
                    ty: record.ty.clone(),
                    field: field.name.to_string(),
                })?;
                write_scalar(&mut out, field.offset, field.ty, value)?;
            }
            Ok(out)
        }
        None => encode_fallback(record),
    }
}

/// 按布局解码一条记录（`layout = None` ⇒ 回退格式）。
pub fn decode_record(ty: &str, bytes: &[u8], layout: Option<Layout>) -> Result<Record, LayoutError> {
    match layout {
        Some(layout) => {
            layout.validate()?;
            if bytes.len() != layout.size {
                return Err(LayoutError::BadLength {
                    ty: ty.to_string(),
                    len: bytes.len(),
                    size: layout.size,
                });
            }
            let mut fields = Vec::with_capacity(layout.fields.len());
            for field in layout.fields {
                fields.push((
                    field.name.to_string(),
                    read_scalar(bytes, field.offset, field.ty)?,
                ));
            }
            Ok(Record::new(ty, fields))
        }
        None => decode_fallback(ty, bytes),
    }
}

fn write_scalar(
    out: &mut [u8],
    offset: usize,
    ty: FieldTy,
    value: Scalar,
) -> Result<(), LayoutError> {
    let bytes: Vec<u8> = match (ty, value) {
        (FieldTy::F32, Scalar::F32(value)) => value.to_bits().to_le_bytes().to_vec(),
        (FieldTy::F64, Scalar::F64(value)) => value.to_bits().to_le_bytes().to_vec(),
        (FieldTy::I64, Scalar::I64(value)) => value.to_le_bytes().to_vec(),
        (FieldTy::Bool, Scalar::Bool(value)) => vec![u8::from(value)],
        (expected, actual) => {
            return Err(LayoutError::UnsupportedScalar(match (expected, actual) {
                (FieldTy::F32, _) => "F32",
                (FieldTy::F64, _) => "F64",
                (FieldTy::I64, _) => "I64",
                (FieldTy::Bool, _) => "Bool",
            }))
        }
    };
    out[offset..offset + bytes.len()].copy_from_slice(&bytes);
    Ok(())
}

fn read_scalar(bytes: &[u8], offset: usize, ty: FieldTy) -> Result<Scalar, LayoutError> {
    let width = ty.width();
    let slice = bytes
        .get(offset..offset + width)
        .ok_or_else(|| LayoutError::CorruptFallback("字段越界".to_string()))?;
    Ok(match ty {
        FieldTy::F32 => Scalar::F32(f32::from_le_bytes(slice.try_into().expect("4 字节"))),
        FieldTy::F64 => Scalar::F64(f64::from_bits(u64::from_le_bytes(
            slice.try_into().expect("8 字节"),
        ))),
        FieldTy::I64 => Scalar::I64(i64::from_le_bytes(
            slice.try_into().expect("8 字节"),
        )),
        FieldTy::Bool => Scalar::Bool(slice[0] != 0),
    })
}

/// 回退格式（无 Layout）：`count:u32` + 每项 `name_len:u16 / name / tag:u8 / payload`。
fn encode_fallback(record: &Record) -> Result<Vec<u8>, LayoutError> {
    let mut out = Vec::new();
    out.extend_from_slice(&(record.fields.len() as u32).to_le_bytes());
    for (name, value) in &record.fields {
        let name = name.as_bytes();
        let len = u16::try_from(name.len())
            .map_err(|_| LayoutError::CorruptFallback("字段名过长".to_string()))?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(name);
        match value {
            Scalar::F32(value) => {
                out.push(3);
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            Scalar::F64(value) => {
                out.push(0);
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            Scalar::I64(value) => {
                out.push(1);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Scalar::Bool(value) => {
                out.push(2);
                out.push(u8::from(*value));
            }
        }
    }
    Ok(out)
}

fn decode_fallback(ty: &str, bytes: &[u8]) -> Result<Record, LayoutError> {
    let mut cursor = 0usize;
    let read = |cursor: &mut usize, width: usize| -> Result<&[u8], LayoutError> {
        let slice = bytes
            .get(*cursor..*cursor + width)
            .ok_or_else(|| LayoutError::CorruptFallback("回退字节流截断".to_string()))?;
        *cursor += width;
        Ok(slice)
    };
    let count = u32::from_le_bytes(read(&mut cursor, 4)?.try_into().expect("4 字节")) as usize;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len =
            u16::from_le_bytes(read(&mut cursor, 2)?.try_into().expect("2 字节")) as usize;
        let name = String::from_utf8(read(&mut cursor, name_len)?.to_vec())
            .map_err(|_| LayoutError::CorruptFallback("字段名不是 UTF-8".to_string()))?;
        let tag = read(&mut cursor, 1)?[0];
        let value = match tag {
            0 => Scalar::F64(f64::from_bits(u64::from_le_bytes(
                read(&mut cursor, 8)?.try_into().expect("8 字节"),
            ))),
            1 => Scalar::I64(i64::from_le_bytes(
                read(&mut cursor, 8)?.try_into().expect("8 字节"),
            )),
            2 => Scalar::Bool(read(&mut cursor, 1)?[0] != 0),
            3 => Scalar::F32(f32::from_bits(u32::from_le_bytes(
                read(&mut cursor, 4)?.try_into().expect("4 字节"),
            ))),
            other => {
                return Err(LayoutError::CorruptFallback(format!(
                    "未知标量标签 {other}"
                )))
            }
        };
        fields.push((name, value));
    }
    Ok(Record::new(ty, fields))
}

/// `zela.Path` 的**慢路径**编码：`count:u32` + `count × (x:f32, y:f32)` + `closed:u8`。
///
/// `zela.Path` 是变长结构 ⇒ 没有 Layout、永远不走零拷贝。这是硬约束 4 的实例。
pub fn encode_path(points: &[[f32; 2]], closed: bool) -> ValueBytes {
    let mut bytes = Vec::with_capacity(4 + points.len() * 8 + 1);
    bytes.extend_from_slice(&(points.len() as u32).to_le_bytes());
    for point in points {
        bytes.extend_from_slice(&point[0].to_le_bytes());
        bytes.extend_from_slice(&point[1].to_le_bytes());
    }
    bytes.push(u8::from(closed));
    ValueBytes::new(crate::layout::PATH_TYPE, bytes)
}

/// `zela.Path` 的慢路径解码。
pub fn decode_path(bytes: &[u8]) -> Result<(Vec<[f32; 2]>, bool), LayoutError> {
    let mut cursor = 0usize;
    let read = |cursor: &mut usize, width: usize| -> Result<&[u8], LayoutError> {
        let slice = bytes
            .get(*cursor..*cursor + width)
            .ok_or_else(|| LayoutError::CorruptFallback("路径字节流截断".to_string()))?;
        *cursor += width;
        Ok(slice)
    };
    let count = u32::from_le_bytes(read(&mut cursor, 4)?.try_into().expect("4 字节")) as usize;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let x = f32::from_le_bytes(read(&mut cursor, 4)?.try_into().expect("4 字节"));
        let y = f32::from_le_bytes(read(&mut cursor, 4)?.try_into().expect("4 字节"));
        points.push([x, y]);
    }
    let closed = read(&mut cursor, 1)?[0] != 0;
    Ok((points, closed))
}

/// `zela.Path` 的注册名。
pub const PATH_TYPE: &str = "zela.Path";

#[cfg(test)]
mod tests {
    use super::*;

    const VEC2_LAYOUT: Layout = Layout::new(8, 4, VEC2_FIELDS);
    const OVERLAP_FIELDS: &[FieldLayout] = &[
        FieldLayout::new("x", 0, FieldTy::F32),
        FieldLayout::new("y", 2, FieldTy::F32),
    ];
    const SWAPPED_FIELDS: &[FieldLayout] = &[
        FieldLayout::new("y", 0, FieldTy::F32),
        FieldLayout::new("x", 4, FieldTy::F32),
    ];
    const SWAPPED_LAYOUT: Layout = Layout::new(8, 4, SWAPPED_FIELDS);

    #[test]
    fn builtin_layouts_validate_and_match_their_sizes() {
        let registry = TypeRegistry::builtin();
        assert_eq!(registry.layout("zela.Vec2"), Some(VEC2_LAYOUT));
        assert_eq!(registry.layout("zela.Vec3").unwrap().size, 12);
        assert!(registry.layout("zela.Path").is_none(), "变长类型没有 Layout");
    }

    #[test]
    fn layout_rejects_overlap_and_bad_align() {
        let overlap = Layout::new(8, 4, OVERLAP_FIELDS);
        assert!(matches!(overlap.validate(), Err(LayoutError::Overlap { .. })));

        let align = Layout::new(8, 3, &[]);
        assert!(matches!(align.validate(), Err(LayoutError::BadAlign { .. })));
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
        assert_eq!(bytes.len(), 8);

        let (same, how) = registry
            .negotiate("zela.Vec2", &bytes, Some(&VEC2_LAYOUT))
            .unwrap();
        assert_eq!(how, Transcode::ZeroCopy);
        assert_eq!(same, bytes, "零拷贝必须逐字节相同");

        // 发送端布局不同（字段顺序不同）⇒ 回退重编码，结果仍与记录一致。
        let swapped = Layout::new(8, 4, SWAPPED_FIELDS);
        let from_swapped = encode_record(&record, Some(swapped)).unwrap();
        let (reencoded, how) = registry
            .negotiate("zela.Vec2", &from_swapped, Some(&swapped))
            .unwrap();
        assert_eq!(how, Transcode::Reencoded);
        let decoded = decode_record("zela.Vec2", &reencoded, registry.layout("zela.Vec2")).unwrap();
        assert_eq!(decoded, record, "布局不一致 ⇒ 回退且结果一致");
    }

    #[test]
    fn path_round_trips_on_the_slow_path() {
        let points = vec![[0.0, 1.0], [2.5, -3.0]];
        let bytes = encode_path(&points, true);
        assert_eq!(bytes.ty, PATH_TYPE);
        let (decoded, closed) = decode_path(&bytes.bytes).unwrap();
        assert_eq!(decoded, points);
        assert!(closed);

        // 两端都没有 Layout（变长类型）⇒ 原样搬运，不尝试按固定布局解码。
        let registry = TypeRegistry::builtin();
        let (same, how) = registry.negotiate_bytes(&bytes, None).unwrap();
        assert_eq!(how, Transcode::Reencoded, "没有 Layout ⇒ 慢路径");
        assert_eq!(same, bytes);
    }

    /// `Value::Bytes` 走同一协商入口：同布局 ⇒ 零拷贝；不同布局 ⇒ 回退重编码。
    #[test]
    fn value_bytes_use_the_same_negotiation_entry() {
        let registry = TypeRegistry::builtin();
        let record = Record::new(
            "zela.Vec2",
            vec![
                ("x".to_string(), Scalar::F32(-1.5)),
                ("y".to_string(), Scalar::F32(2.25)),
            ],
        );
        let canonical = ValueBytes::new(
            "zela.Vec2",
            encode_record(&record, Some(VEC2_LAYOUT)).unwrap(),
        );
        let (same, how) = registry.negotiate_bytes(&canonical, Some(&VEC2_LAYOUT)).unwrap();
        assert_eq!(how, Transcode::ZeroCopy);
        assert_eq!(same, canonical);

        // 发送端用的是"字段顺序不同"的布局 ⇒ 回退重编码到本地布局。
        let foreign = ValueBytes::new(
            "zela.Vec2",
            encode_record(&record, Some(SWAPPED_LAYOUT)).unwrap(),
        );
        let (reencoded, how) = registry.negotiate_bytes(&foreign, Some(&SWAPPED_LAYOUT)).unwrap();
        assert_eq!(how, Transcode::Reencoded);
        let decoded = decode_record(
            "zela.Vec2",
            &reencoded.bytes,
            registry.layout("zela.Vec2"),
        )
        .unwrap();
        assert_eq!(decoded.get("x"), Some(Scalar::F32(-1.5)));
        assert_eq!(decoded.get("y"), Some(Scalar::F32(2.25)));
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
        assert!(matches!(
            registry.register("acme.Tensor", None),
            Err(LayoutError::DuplicateType(_))
        ));
    }
}
