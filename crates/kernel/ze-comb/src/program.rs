//! 求值：**装配期建图 → 运行期定序**，效果只能从 [`Mono`] 进。
//!
//! # 为什么不复用 `LazyPullEval`
//!
//! `LazyPullEval` 把状态放在**求值器自己**里（`&mut self` + Mutex 缓存）⇒ 不可枚举、
//! 不可结晶。这里的状态只在 `Mono`（宿主注入的 M）里 ⇒ 可枚举、可结晶。
//! 运行时缓存只能作为**优化**存在，且必须能证明与「朴素重放 M」逐位等价。
//!
//! # 形状
//!
//! [`CompiledExpr`] 是一段**后序程序**：每个 `Step` 的实参索引都小于自身 ⇒ 求值就是
//! 一次线性扫描，无需运行时拓扑排序、无需脏传播。
//!
//! - 共享节点（带 `id` 的 `Def` + `Ref`）**只编译一次** ⇒ 帧内只求一次、广播给引用者；
//! - 程序本身**无内部可变性** ⇒ 每一帧都是重新扫描，不存在跨帧缓存；
//! - 清空缓存重算逐位相同 —— 因为根本没有缓存可清。
//!
//! # 不纯值
//!
//! 字面量 / 通道 / 算子之外没有别的叶子。通道由 [`Mono::read`] 提供，算子由
//! [`Mono::call`] 提供 —— **宿主是唯一的不纯入口**，且它的实现可枚举。

use std::collections::BTreeMap;

use ze_type::ScalarKind;

use crate::expr::{ChannelRef, Expr, ExprError, Literal, MethodRef, Node, OpRef};

/// 标量 / 向量的**最多分量数**（`Vec4` 是当前上限）。
pub const SCALAR_MAX_DIM: u8 = 4;

/// 单个分量的最大字节宽度（`f64` / `i64` / `u64`）。
const MAX_LANE_BYTES: usize = 8;

/// 定长数据区（`dim × lane`，**零堆分配**）。
const SCALAR_DATA_BYTES: usize = SCALAR_MAX_DIM as usize * MAX_LANE_BYTES;

/// 运行时值（跨通道 / 算子的最小集）。
///
/// # 为什么标量与向量是**一个**变体
///
/// 旧形态 `F64 | I64 | Vec2 | Vec3` 是同一件事的四种组合（`ty × dim`）各占一格：
/// 加一个 `F32` 标量或 `Vec4` 就得多一个变体，而每个 `match` 都要跟着改。
/// 现在 `Scalar { ty, dim }` 与类型侧的 [`ze_type::TypeSpec`]（`Scalar` / `App`）同构，
/// **值侧对齐类型侧**。
///
/// # 精度口径（风险表第 4 条的显式裁决）
///
/// 标量取 `F64`、向量取 `F32` —— 这不是"不一致被藏起来"，而是**显式的两种类型**：
/// 向量与 `ze-type::layout` 的 `zela.Vec2/Vec3`（`f32` 字段）逐位一致，标量与
/// 表达式层的 `f64` 算术逐位一致。统一成单一宽度会改变像素基线（禁止）。
///
/// `Path` 已**迁移出核心**（阶段 1）：路径是 `Bytes` + 命名类型 `zela.Path`，
/// 内核只认识"带类型名的字节块"，不认识"路径"这个领域概念。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Value {
    /// 标量 / 向量：**`ty × dim` 参数化**（`F64×1` = 数、`I64×1` = 离散索引、
    /// `F32×2/3/4` = 向量）。
    Scalar(Scalar),
    Bool(bool),
    /// **字节块 + 命名类型**（`zela.Path` / `zela.Text` / 第三方私有类型）。
    ///
    /// 命名类型与布局描述在 `ze-func::layout` 的注册表里；本 crate 只搬运字节。
    ///
    /// **阶段 2**：文本不再是独立变体 —— 它是 `Bytes + zela.Text`（变长 ⇒ 没有
    /// `Layout` ⇒ 永远慢路径）。下沉的意义是让文本与 `zela.Path` / `acme.Tensor`
    /// 走**同一套**协商机制，而不是"看起来快"。
    Bytes(ValueBytes),
}

/// 文本的命名类型（阶段 2：`Text` 下沉为 `Bytes + zela.Text`）。
pub const TEXT_TYPE: &str = "zela.Text";

/// 标量 / 向量：`ty`（分量类型）× `dim`（分量数）的定长值。
///
/// 字段私有 ⇒ **只能**经构造函数产生（分量宽度与 `dim` 的一致性由入口保证，
/// 见 [`Scalar::f32_vec`]）：这让 `match (ty, dim)` 的分支可以被信任。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scalar {
    ty: ScalarKind,
    dim: u8,
    /// 分量按 `ty` 的宽度小端连续存放；未用字节恒为 0（`PartialEq` 依赖这一点）。
    data: [u8; SCALAR_DATA_BYTES],
}

impl Scalar {
    /// `f64` 标量（表达式层的通用数值）。
    pub fn f64(value: f64) -> Self {
        Self::single(ScalarKind::F64, value.to_le_bytes())
    }

    /// `i64` 离散值（递归索引 / 帧号；**不经过 f64**）。
    pub fn i64(value: i64) -> Self {
        Self::single(ScalarKind::I64, value.to_le_bytes())
    }

    /// `f32` 向量（`dim ∈ 1..=4`，即 `F32` 标量 / `Vec2` / `Vec3` / `Vec4`）。
    pub fn f32_vec(components: &[f32]) -> Self {
        let dim = components.len();
        assert!(
            (1..=SCALAR_MAX_DIM as usize).contains(&dim),
            "向量分量数必须在 1..=4，实际 {dim}"
        );
        let mut data = [0u8; SCALAR_DATA_BYTES];
        for (index, component) in components.iter().enumerate() {
            let offset = index * 4;
            data[offset..offset + 4].copy_from_slice(&component.to_le_bytes());
        }
        Self {
            ty: ScalarKind::F32,
            dim: dim as u8,
            data,
        }
    }

    fn single(ty: ScalarKind, lane: [u8; MAX_LANE_BYTES]) -> Self {
        let mut data = [0u8; SCALAR_DATA_BYTES];
        data[..MAX_LANE_BYTES].copy_from_slice(&lane);
        Self { ty, dim: 1, data }
    }

    /// 分量类型。
    pub fn ty(&self) -> ScalarKind {
        self.ty
    }

    /// 分量数（`1` = 标量）。
    pub fn dim(&self) -> u8 {
        self.dim
    }

    /// 数值视图：**只有** `F64 × 1` 是"表达式层的数"（不做隐式提升）。
    pub fn as_f64(&self) -> Option<f64> {
        match (self.ty, self.dim) {
            (ScalarKind::F64, 1) => Some(f64::from_le_bytes(self.lane(0))),
            _ => None,
        }
    }

    /// 整数视图：**只有** `I64 × 1`（不把浮点静默取整）。
    pub fn as_i64(&self) -> Option<i64> {
        match (self.ty, self.dim) {
            (ScalarKind::I64, 1) => Some(i64::from_le_bytes(self.lane(0))),
            _ => None,
        }
    }

    /// `F32` 向量的第 `index` 个分量（`index < dim` 且 `ty == F32`）。
    pub fn lane_f32(&self, index: usize) -> Option<f32> {
        if self.ty != ScalarKind::F32 || index >= usize::from(self.dim) {
            return None;
        }
        let offset = index * 4;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[offset..offset + 4]);
        Some(f32::from_le_bytes(bytes))
    }

    /// `F32 × 2` 视图（`Vec2`）。
    pub fn as_vec2(&self) -> Option<[f32; 2]> {
        if (self.ty, self.dim) != (ScalarKind::F32, 2) {
            return None;
        }
        Some([self.lane_f32(0)?, self.lane_f32(1)?])
    }

    /// `F32 × 3` 视图（`Vec3`）。
    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        if (self.ty, self.dim) != (ScalarKind::F32, 3) {
            return None;
        }
        Some([
            self.lane_f32(0)?,
            self.lane_f32(1)?,
            self.lane_f32(2)?,
        ])
    }

    /// `F32 × 4` 视图（`Vec4`）。
    pub fn as_vec4(&self) -> Option<[f32; 4]> {
        if (self.ty, self.dim) != (ScalarKind::F32, 4) {
            return None;
        }
        Some([
            self.lane_f32(0)?,
            self.lane_f32(1)?,
            self.lane_f32(2)?,
            self.lane_f32(3)?,
        ])
    }

    /// 值种类名（错误消息 / 诊断用）。
    fn kind_name(&self) -> &'static str {
        match self.ty {
            ScalarKind::F64 => "scalar",
            ScalarKind::I64 => "i64",
            ScalarKind::F32 => match self.dim {
                1 => "f32",
                2 => "vec2",
                3 => "vec3",
                _ => "vec4",
            },
            ScalarKind::I32 => "i32",
            ScalarKind::U32 => "u32",
            ScalarKind::U64 => "u64",
            ScalarKind::U8 => "u8",
            // 构造函数已拒绝这两个：`Bool` 有独立变体，`String` 是变长（阶段 2 下沉 Bytes）。
            ScalarKind::Bool | ScalarKind::String => {
                panic!("Scalar 不承载 {ty:?}（Bool 走 Value::Bool，String 走 Value::Bytes + zela.Text）", ty = self.ty)
            }
        }
    }

    /// 分量宽度（**编译期穷尽**：新增 `ScalarKind` 必须在这里决定能否做标量载荷）。
    const fn lane_width(ty: ScalarKind) -> Option<usize> {
        match ty {
            ScalarKind::F32 | ScalarKind::I32 | ScalarKind::U32 => Some(4),
            ScalarKind::F64 | ScalarKind::I64 | ScalarKind::U64 => Some(8),
            ScalarKind::U8 => Some(1),
            ScalarKind::Bool | ScalarKind::String => None,
        }
    }

    /// 分量种类在**规范字节**里的编码（新增种类必须给码，否则编译失败）。
    const fn ty_code(ty: ScalarKind) -> u8 {
        match ty {
            ScalarKind::F32 => 0,
            ScalarKind::F64 => 1,
            ScalarKind::I32 => 2,
            ScalarKind::I64 => 3,
            ScalarKind::U32 => 4,
            ScalarKind::U64 => 5,
            ScalarKind::U8 => 6,
            ScalarKind::Bool => 7,
            ScalarKind::String => 8,
        }
    }

    fn lane(&self, index: usize) -> [u8; MAX_LANE_BYTES] {
        let width = Self::lane_width(self.ty).expect("Scalar 的 ty 必须是标量载荷");
        let offset = index * width;
        let mut lane = [0u8; MAX_LANE_BYTES];
        lane[..width].copy_from_slice(&self.data[offset..offset + width]);
        lane
    }

    /// 实际使用的字节（错误信息 / 序列化用；不暴露填充）。
    fn used_bytes(&self) -> &[u8] {
        let width = Self::lane_width(self.ty).expect("Scalar 的 ty 必须是标量载荷");
        &self.data[..usize::from(self.dim) * width]
    }

    /// **规范字节编码**（与既有标签**逐位**一致）。
    ///
    /// 旧形态的标签必须保留：值序列哈希（`hash_values`）是基线的一部分，
    /// 改编码就是改基线。新组合（`Vec4` / `F32` 标量 / 其它 `ScalarKind`）走
    /// 通用标签 `7`：`ty_code + dim + 分量字节`。
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match (self.ty, self.dim) {
            (ScalarKind::F64, 1) => {
                out.push(0);
                out.extend_from_slice(self.used_bytes());
            }
            (ScalarKind::I64, 1) => {
                out.push(6);
                out.extend_from_slice(self.used_bytes());
            }
            (ScalarKind::F32, 2) => {
                out.push(3);
                out.extend_from_slice(self.used_bytes());
            }
            (ScalarKind::F32, 3) => {
                out.push(4);
                out.extend_from_slice(self.used_bytes());
            }
            _ => {
                out.push(7);
                out.push(Self::ty_code(self.ty));
                out.push(self.dim);
                out.extend_from_slice(self.used_bytes());
            }
        }
        out
    }
}

impl serde::Serialize for Scalar {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Scalar", 3)?;
        state.serialize_field("ty", kind_code(self.ty))?;
        state.serialize_field("dim", &self.dim)?;
        state.serialize_field("data", self.used_bytes())?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for Scalar {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Wire {
            ty: String,
            dim: u8,
            data: Vec<u8>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let ty = kind_from_code(&wire.ty).ok_or_else(|| {
            serde::de::Error::custom(format!("未知标量种类 `{}`", wire.ty))
        })?;
        let width = Scalar::lane_width(ty)
            .ok_or_else(|| serde::de::Error::custom(format!("标量种类 `{}` 不能承载值", wire.ty)))?;
        if wire.dim == 0 || wire.dim > SCALAR_MAX_DIM {
            return Err(serde::de::Error::custom(format!(
                "标量维度必须在 1..=4，实际 {}",
                wire.dim
            )));
        }
        let expected = usize::from(wire.dim) * width;
        if wire.data.len() != expected {
            return Err(serde::de::Error::custom(format!(
                "标量数据长度 {} 与 {width} 字节 × {} 个分量不符",
                wire.data.len(),
                wire.dim
            )));
        }
        let mut data = [0u8; SCALAR_DATA_BYTES];
        data[..expected].copy_from_slice(&wire.data);
        Ok(Scalar {
            ty,
            dim: wire.dim,
            data,
        })
    }
}

/// 种类在序列化里的短名（小写，与 `kind_name` 的前缀一致）。
fn kind_code(ty: ScalarKind) -> &'static str {
    match ty {
        ScalarKind::F32 => "f32",
        ScalarKind::F64 => "f64",
        ScalarKind::I32 => "i32",
        ScalarKind::I64 => "i64",
        ScalarKind::U32 => "u32",
        ScalarKind::U64 => "u64",
        ScalarKind::U8 => "u8",
        ScalarKind::Bool => "bool",
        ScalarKind::String => "string",
    }
}

fn kind_from_code(code: &str) -> Option<ScalarKind> {
    Some(match code {
        "f32" => ScalarKind::F32,
        "f64" => ScalarKind::F64,
        "i32" => ScalarKind::I32,
        "i64" => ScalarKind::I64,
        "u32" => ScalarKind::U32,
        "u64" => ScalarKind::U64,
        "u8" => ScalarKind::U8,
        "bool" => ScalarKind::Bool,
        "string" => ScalarKind::String,
        _ => return None,
    })
}

/// 带类型名的字节块。
#[derive(
    Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub struct ValueBytes {
    /// 命名类型（`zela.Path` / `acme.Tensor` …）。
    pub ty: String,
    pub bytes: Vec<u8>,
}

impl ValueBytes {
    pub fn new(ty: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            ty: ty.into(),
            bytes: bytes.into(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Value {
    /// `f64` 标量（表达式层的通用数值）。
    pub fn f64(value: f64) -> Self {
        Self::Scalar(Scalar::f64(value))
    }

    /// `i64` 离散值（递归索引 / 帧号）。
    pub fn i64(value: i64) -> Self {
        Self::Scalar(Scalar::i64(value))
    }

    /// `Vec2`（`F32 × 2`）。
    pub fn vec2(value: [f32; 2]) -> Self {
        Self::Scalar(Scalar::f32_vec(&value))
    }

    /// `Vec3`（`F32 × 3`）。
    pub fn vec3(value: [f32; 3]) -> Self {
        Self::Scalar(Scalar::f32_vec(&value))
    }

    /// `Vec4`（`F32 × 4`）。
    pub fn vec4(value: [f32; 4]) -> Self {
        Self::Scalar(Scalar::f32_vec(&value))
    }

    /// 标量 / 向量视图（`Bool` / `Text` / `Bytes` 返回 `None`）。
    pub fn scalar(&self) -> Option<&Scalar> {
        match self {
            Value::Scalar(scalar) => Some(scalar),
            _ => None,
        }
    }

    /// 取数值（**只有** `F64 × 1`；`F32` / `I64` 不做隐式提升）。
    pub fn as_f64(&self) -> Option<f64> {
        self.scalar().and_then(Scalar::as_f64)
    }

    /// 取整数（**只有** `I64 × 1`；不把 f64 静默取整）。
    pub fn as_i64(&self) -> Option<i64> {
        self.scalar().and_then(Scalar::as_i64)
    }

    /// 取布尔。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// 文本值（`Bytes + zela.Text`，UTF-8）。
    pub fn text(text: impl Into<String>) -> Self {
        Self::Bytes(ValueBytes::new(TEXT_TYPE, text.into().into_bytes()))
    }

    /// 取文本：只有命名类型是 `zela.Text` **且**字节是合法 UTF-8 时成立。
    ///
    /// 第三方若用 `zela.Text` 塞了非 UTF-8 字节，这里返回 `None`（不静默替换）。
    pub fn as_text(&self) -> Option<&str> {
        let bytes = self.as_bytes()?;
        if bytes.ty != TEXT_TYPE {
            return None;
        }
        std::str::from_utf8(&bytes.bytes).ok()
    }

    /// 取字节块。
    pub fn as_bytes(&self) -> Option<&ValueBytes> {
        match self {
            Value::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// `Vec2` 视图。
    pub fn as_vec2(&self) -> Option<[f32; 2]> {
        self.scalar().and_then(Scalar::as_vec2)
    }

    /// `Vec3` 视图。
    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        self.scalar().and_then(Scalar::as_vec3)
    }

    /// `Vec4` 视图。
    pub fn as_vec4(&self) -> Option<[f32; 4]> {
        self.scalar().and_then(Scalar::as_vec4)
    }

    /// 值种类名（错误消息 / 诊断用）。
    pub fn kind(&self) -> &'static str {
        value_kind(self)
    }

    /// **规范字节编码**：跨进程 / 跨语言对拍用的稳定表示。
    ///
    /// 浮点按 IEEE-754 位模式（小端）编码 —— 逐位可比，不做 `to_string` 的精度转换。
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Value::Scalar(scalar) => out.extend(scalar.canonical_bytes()),
            Value::Bool(value) => {
                out.push(1);
                out.push(u8::from(*value));
            }
            Value::Bytes(bytes) => {
                // **文本保留既有标签 2**：`zela.Text` 的规范编码与阶段 1 之前的
                // `Value::Text` 逐位相同 ⇒ 值序列哈希不动（其余命名类型走标签 5）。
                if bytes.ty == TEXT_TYPE {
                    out.push(2);
                    out.extend_from_slice(&(bytes.bytes.len() as u32).to_le_bytes());
                    out.extend_from_slice(&bytes.bytes);
                } else {
                    out.push(5);
                    out.extend_from_slice(&(bytes.ty.len() as u32).to_le_bytes());
                    out.extend_from_slice(bytes.ty.as_bytes());
                    out.extend_from_slice(&(bytes.bytes.len() as u32).to_le_bytes());
                    out.extend_from_slice(&bytes.bytes);
                }
            }
        }
        out
    }
}

/// 一串值的**规范哈希**（FNV-1a 64；fixture 的"期望值序列哈希"）。
///
/// 选择 FNV-1a 不是为了抗碰撞，而是为了**零依赖、跨语言十行可复现**：
/// JS / Lua 的 fixture runner 用同一份常量即可得到同一个数。
pub fn hash_values(values: &[Value]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in values {
        for byte in value.canonical_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 单子 `M`：宿主注入的**唯一效果入口**。
///
/// 实现方必须可枚举（时间 / 帧 / 事件 / 组件 / 环境），且 `call` 只做纯函数。
pub trait Mono {
    /// 读取一条通道。
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError>;

    /// 调用一个叶子算子。
    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError>;

    /// 读**某对象**上的组件字段（G 批通道读端）。
    ///
    /// `object` 是 `ObjectId` 的打包值（`index:u32<<32 | generation:u32`；
    /// ze-comb 是 kernel 层，不依赖 `ze-scene-contract`）。默认实现**显式失败**
    /// （`MissingChannel`）：宿主没实现读端就不该静默返回 0 ——
    /// 既有 `Mono` 实现（如测试里的 `ScalarMono`）保持默认，不破。
    fn read_object(&self, channel: &ChannelRef, _object: u64) -> Result<Value, EvalError> {
        Err(EvalError::MissingChannel {
            channel: channel.clone(),
        })
    }

    /// 调用**某对象**上的方法（B2）。
    ///
    /// 默认实现**显式失败**（`MissingMethod`）：宿主没注册这个方法就不该返回值。
    fn call_method(
        &self,
        method: &MethodRef,
        _object: u64,
        _args: &[Value],
    ) -> Result<Value, EvalError> {
        Err(EvalError::MissingMethod {
            method: method.qualified(),
        })
    }
}

/// 求值错误：显式、带算子来源（§7.6 的运行时对应物）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// 通道没有值 —— 说明宿主没有注入它（不是静默 0）。
    #[error("通道 `{channel}` 未由宿主注入")]
    MissingChannel { channel: ChannelRef },
    /// 对象上不存在通道对应的组件（地址已注册但该对象没挂这个组件）。
    #[error("对象 {object} 上没有通道 `{channel}` 对应的组件")]
    MissingComponent { object: u64, channel: ChannelRef },
    /// 算子求值失败（类型 / 定义域 / 未知算子）。
    #[error("算子 `{op}` 求值失败：{message}")]
    Op { op: String, message: String },
    /// `Select` 的条件不是 `Bool`（显式失败，不静默当真 / 假）。
    #[error("select 的条件必须是 Bool，实际 {actual}")]
    SelectCondition { actual: &'static str },
    /// 通道**声明类型**与运行期返回值标签不符（装配期推导 vs 实际值对账）。
    #[error("通道 `{channel}` 声明为 {expected}，运行期返回 {actual}")]
    ChannelTypeMismatch {
        channel: ChannelRef,
        expected: &'static str,
        actual: &'static str,
    },
    /// 组件的值类型没有表达式层表示（如 `Color`）。
    ///
    /// 以前是 `anim_to_value → None → 被当成 MissingComponent`（**错误的静默**）；
    /// 现在显式点名"这个值类型不支持读"。
    #[error("通道 `{channel}` 的值类型 {kind} 没有表达式层表示")]
    UnsupportedValue {
        channel: ChannelRef,
        kind: &'static str,
    },
    /// `NatRec.count` 不是非负 `I64`（硬约束 6：禁止刀刃取整）。
    #[error("NatRec.count 必须是 I64，实际 {actual}")]
    RecCount { actual: &'static str },
    /// `NatRec` 的轮数超过**动态护栏**（库级默认；只有影响语义的才进产物）。
    #[error("NatRec 轮数 {count} 超过上限 {max_steps}（不收敛/过大 ⇒ 显式错误）")]
    RecBudget { count: i64, max_steps: i64 },
    /// 方法没有由宿主注册（未知方法 ⇒ 显式失败，不静默给 0）。
    #[error("方法 `{method}` 未由宿主注册")]
    MissingMethod { method: String },
    /// 退化 / 空几何没有包围盒（`None` 贯穿到底，不拿零矩形冒充）。
    #[error("对象 {object} 没有可用的几何包围盒")]
    NoGeometry { object: u64 },
    /// 递归叶子出现在没有 `NatRec` 上下文的地方（装配期应已拒绝）。
    #[error("递归叶子 `{leaf}` 不在 NatRec.step 求值上下文里")]
    RecContext { leaf: &'static str },
    /// 递归状态类型在折叠过程中变了（静默换类型会让结果不可解释）。
    #[error("NatRec 状态类型不一致：期望 {expected}，第 {index} 步得到 {actual}")]
    RecStateType {
        expected: &'static str,
        actual: &'static str,
        index: i64,
    },
    /// `exists` 没有在装配期折叠（对象集只有装配期才知道）⇒ 显式失败。
    #[error("`exists` 必须在装配期折叠成常量（求值期没有对象集）")]
    UnfoldedExists,
}

impl EvalError {
    /// 构造一条带算子限定名的错误。
    pub fn op(op: &OpRef, message: impl Into<String>) -> Self {
        Self::Op {
            op: op.qualified(),
            message: message.into(),
        }
    }
}

/// `NatRec` 的**显式缓存**（阶段 3）：状态在参数里，可序列化、可清空。
///
/// `count` 是已经折叠到的索引（`0` = 只有 `init`）；`value` 是该索引处的状态。
/// 清空缓存重算 = `None` ⇒ 从 `init` 开始逐位重放 —— "逐位相同"因此是**结构事实**，
/// 而不是纪律。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecCache {
    /// 已折叠到的索引（非负）。
    pub count: i64,
    pub value: Value,
}

/// `NatRec` 轮数的**库级默认 + 动态护栏**（不进产物：它不改变语义，只拒绝失控）。
pub const REC_MAX_STEPS: i64 = 1_000_000;

/// 一次求值的递归上下文。
#[derive(Clone, Debug, Default)]
struct EvalContext {
    /// 当前 `NatRec.step` 的 `(上一步状态, 索引)`。
    rec: Option<RecFrame>,
    /// 根级 `NatRec` 的显式缓存（只在外层程序使用）。
    cache: Option<RecCache>,
    /// 是否允许读写缓存（子程序内关闭，避免污染）。
    cache_enabled: bool,
}

#[derive(Clone, Debug)]
struct RecFrame {
    prev: Value,
    index: i64,
}

/// 后序程序的一步。
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Pure(Literal),
    Channel(ChannelRef),
    /// 读**某对象**的组件字段（G 批通道读端）。
    ///
    /// `object` 是 `ObjectId` 打包值：`commit` 时场景层把 `Node::Read.object`
    /// （脚本局部句柄下标）重写为 `index:u32<<32 | generation:u32` 再编译，
    /// 运行期经 [`Mono::read_object`] 解包读组件。
    Read { channel: ChannelRef, object: u64 },
    /// **方法调用**（B2）：实参在 `args` 里，`object` 是对象打包值。
    Method {
        method: MethodRef,
        object: u64,
        args: Vec<usize>,
    },
    Call { op: OpRef, args: Vec<usize> },
    /// **值分支**（H 批）：`cond` 是外层程序里的索引，两个分支各自是**独立子程序**。
    ///
    /// 子程序形态让"惰性"成为结构性事实：只有被选中的那一支会被 `eval`，
    /// 另一支的通道 / 算子一次都不求值（不是靠短路标记）。
    Select {
        cond: usize,
        then: Box<CompiledExpr>,
        r#else: Box<CompiledExpr>,
    },
    /// **结构递归**（阶段 3）：`count` 是外层程序里的索引（必须是 `I64`），
    /// `init` / `step` 是独立子程序。`step` 内的 `RecPrev` / `RecIndex` 由
    /// [`EvalContext`] 提供。
    NatRec {
        count: usize,
        init: Box<CompiledExpr>,
        step: Box<CompiledExpr>,
    },
    /// `NatRec.step` 内：上一步状态。
    RecPrev,
    /// `NatRec.step` 内：当前索引（`I64`）。
    RecIndex,
    /// `exists` 漏折到运行期（装配期必须折叠成常量）⇒ 求值即失败。
    UnfoldedExists,
}

/// 编译后的表达式（装配期一次，运行期线性扫描）。
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledExpr {
    steps: Vec<Step>,
    root: usize,
    /// 是否含**离散层**节点（`NatRec` / `RecPrev` / `RecIndex`）。
    ///
    /// 这是 `infer::PointwiseReport` 在 kernel 侧的投影：含离散层 ⇒ 求值需要历史
    /// 或整段重放，不能按"单帧纯函数"寻址（`SeekKind::Replay`）。
    requires_replay: bool,
}

impl CompiledExpr {
    /// 编译并校验（悬空 `Ref` / 环 / 重名 id 都在这里显式失败）。
    pub fn compile(expr: &Expr) -> Result<Self, ExprError> {
        expr.validate()?;
        let defs = crate::expr::definitions(expr);
        let mut steps = Vec::new();
        let mut memo = BTreeMap::new();
        let root = compile_node(expr, &defs, &mut memo, &mut steps);
        Ok(Self {
            steps,
            root,
            requires_replay: expr.requires_replay(),
        })
    }

    /// 是否含离散层节点（`NatRec` 及其递归叶子）。
    pub fn requires_replay(&self) -> bool {
        self.requires_replay
    }

    /// 程序步数（共享节点只计一次）。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 程序（只读；用于诊断 / 导出）。
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// 求值一次。
    ///
    /// 每次调用都从头扫描 ⇒ **帧内 memo 由程序结构决定，不跨帧**。
    pub fn eval(&self, mono: &dyn Mono) -> Result<Value, EvalError> {
        let mut context = EvalContext::default();
        self.eval_with(mono, &mut context)
    }

    /// **阶段 3**：带显式 `NatRec` 缓存的求值。
    ///
    /// - `cache = None` ⇒ 从 `init` 开始逐位重放；
    /// - `cache = Some(RecCache { count, value })` 且 `count ≤ 目标轮数` ⇒ 从该状态续算；
    /// - 缓存**可序列化**；清空后重算逐位相同（结构事实）。
    pub fn eval_cached(
        &self,
        mono: &dyn Mono,
        cache: &mut Option<RecCache>,
    ) -> Result<Value, EvalError> {
        let mut context = EvalContext {
            rec: None,
            cache: cache.take(),
            cache_enabled: true,
        };
        let value = self.eval_with(mono, &mut context);
        *cache = context.cache;
        value
    }

    fn eval_with(&self, mono: &dyn Mono, context: &mut EvalContext) -> Result<Value, EvalError> {
        let mut values: Vec<Value> = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let value = match step {
                Step::Pure(literal) => literal_value(literal),
                Step::Channel(channel) => mono.read(channel)?,
                Step::Read { channel, object } => mono.read_object(channel, *object)?,
                Step::Select { cond, then, r#else } => match values[*cond].as_bool() {
                    Some(true) => then.eval_with(mono, context)?,
                    Some(false) => r#else.eval_with(mono, context)?,
                    None => {
                        return Err(EvalError::SelectCondition {
                            actual: value_kind(&values[*cond]),
                        })
                    }
                },
                Step::NatRec { count, init, step } => {
                    let count = match values[*count].as_i64() {
                        Some(value) if value >= 0 => value,
                        _ => {
                            return Err(EvalError::RecCount {
                                actual: value_kind(&values[*count]),
                            })
                        }
                    };
                    if count > REC_MAX_STEPS {
                        return Err(EvalError::RecBudget {
                            count,
                            max_steps: REC_MAX_STEPS,
                        });
                    }
                    // 根级显式缓存：**只有一个** NatRec 拥有它（第一个被求值的）。
                    // 子程序内关闭，避免污染；拥有后立刻关闭，避免第二个 NatRec
                    // 误用别人的状态。
                    let owns_cache = context.cache_enabled;
                    let cached = if owns_cache {
                        context.cache_enabled = false;
                        context.cache.take()
                    } else {
                        None
                    };
                    let (mut state, mut index) = match cached {
                        Some(cache) if cache.count <= count => (cache.value, cache.count),
                        _ => (init.eval_with(mono, &mut EvalContext::default())?, 0),
                    };
                    let state_kind = value_kind(&state);
                    while index < count {
                        index += 1;
                        let mut step_context = EvalContext {
                            rec: Some(RecFrame {
                                prev: state.clone(),
                                index,
                            }),
                            cache: None,
                            cache_enabled: false,
                        };
                        let next = step.eval_with(mono, &mut step_context)?;
                        if value_kind(&next) != state_kind {
                            return Err(EvalError::RecStateType {
                                expected: state_kind,
                                actual: value_kind(&next),
                                index,
                            });
                        }
                        state = next;
                    }
                    if owns_cache {
                        context.cache = Some(RecCache {
                            count,
                            value: state.clone(),
                        });
                    }
                    state
                }
                Step::RecPrev => match &context.rec {
                    Some(frame) => frame.prev.clone(),
                    None => return Err(EvalError::RecContext { leaf: "rec_prev" }),
                },
                Step::RecIndex => match &context.rec {
                    Some(frame) => Value::i64(frame.index),
                    None => return Err(EvalError::RecContext { leaf: "rec_index" }),
                },
                Step::UnfoldedExists => return Err(EvalError::UnfoldedExists),
                Step::Method {
                    method,
                    object,
                    args,
                } => {
                    let args: Vec<Value> = args.iter().map(|&index| values[index].clone()).collect();
                    mono.call_method(method, *object, &args)?
                }
                Step::Call { op, args } => {
                    let args: Vec<Value> = args.iter().map(|&index| values[index].clone()).collect();
                    mono.call(op, &args)?
                }
            };
            values.push(value);
        }
        Ok(values[self.root].clone())
    }
}

/// 值种类名（错误消息用；与 Pod 层词汇一致）。
pub(crate) fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Scalar(scalar) => scalar.kind_name(),
        Value::Bool(_) => "bool",
        // 文本是 `Bytes + zela.Text`：诊断词汇仍叫 `text`（与通道 / 错误消息一致）。
        Value::Bytes(bytes) if bytes.ty == TEXT_TYPE => "text",
        Value::Bytes(_) => "bytes",
    }
}

pub(crate) fn literal_value(literal: &Literal) -> Value {
    match literal {
        Literal::F32(value) => Value::f64(f64::from(*value)),
        Literal::F64(value) => Value::f64(*value),
        Literal::I32(value) => Value::f64(f64::from(*value)),
        Literal::I64(value) => Value::i64(*value),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Text(value) => Value::text(value.clone()),
        Literal::Vec2(value) => Value::vec2(*value),
        Literal::Vec3(value) => Value::vec3(*value),
    }
}

fn compile_node<'a>(
    expr: &'a Expr,
    defs: &BTreeMap<&'a str, &'a Expr>,
    memo: &mut BTreeMap<&'a str, usize>,
    steps: &mut Vec<Step>,
) -> usize {
    if let Some(id) = expr.id.as_deref() {
        if let Some(index) = memo.get(id) {
            return *index;
        }
    }

    let index = match &expr.node {
        Node::Pure { value } => {
            steps.push(Step::Pure(value.clone()));
            steps.len() - 1
        }
        Node::Channel { channel } => {
            steps.push(Step::Channel(channel.clone()));
            steps.len() - 1
        }
        Node::Read { channel, object } => {
            // 把 `object` 原样放入 Step（声明期 = 脚本局部句柄下标；commit 时
            // 场景层已把 Expr 树里的 `Node::Read.object` 重写为 ObjectId 打包值，
            // 见 `Timeline::rewrite_read_objects` —— 两种语义对 `u64` 都成立）。
            steps.push(Step::Read {
                channel: channel.clone(),
                object: *object,
            });
            steps.len() - 1
        }
        Node::Method {
            method,
            object,
            args,
        } => {
            let args: Vec<usize> = args
                .iter()
                .map(|arg| compile_node(arg, defs, memo, steps))
                .collect();
            steps.push(Step::Method {
                method: method.clone(),
                object: *object,
                args,
            });
            steps.len() - 1
        }
        Node::Call { op, args } => {
            let args: Vec<usize> = args
                .iter()
                .map(|arg| compile_node(arg, defs, memo, steps))
                .collect();
            steps.push(Step::Call {
                op: op.clone(),
                args,
            });
            steps.len() - 1
        }
        Node::Ref { to } => {
            // `validate` 已拒绝悬空 Ref；这里保留 expect 作为内部不变量。
            let target = defs
                .get(to.as_str())
                .expect("compile 之前必须通过 Expr::validate");
            compile_node(target, defs, memo, steps)
        }
        // **H 批**：`cond` 编进外层程序（它的值就在合并点前一步），
        // 两个分支各自编译成**独立子程序**（各自的值表 / 根索引），
        // 运行期只 `eval` 被选中那一支 ⇒ 惰性是结构性的。
        Node::Select { cond, then, r#else } => {
            let cond = compile_node(cond, defs, memo, steps);
            let then = compile_branch(then, defs);
            let r#else = compile_branch(r#else, defs);
            steps.push(Step::Select {
                cond,
                then: Box::new(then),
                r#else: Box::new(r#else),
            });
            steps.len() - 1
        }
        // **阶段 3**：`count` 编进外层；`init` / `step` 各自独立子程序
        // （`step` 内的 `RecPrev` / `RecIndex` 是普通叶子，编译成专用 `Step`）。
        Node::NatRec { count, init, step } => {
            let count = compile_node(count, defs, memo, steps);
            let init = compile_branch(init, defs);
            let step = compile_branch(step, defs);
            steps.push(Step::NatRec {
                count,
                init: Box::new(init),
                step: Box::new(step),
            });
            steps.len() - 1
        }
        Node::RecPrev => {
            steps.push(Step::RecPrev);
            steps.len() - 1
        }
        Node::RecIndex => {
            steps.push(Step::RecIndex);
            steps.len() - 1
        }
        // `exists` 只可能在装配期折叠；漏下来就在求值期显式失败。
        Node::Exists { .. } => {
            steps.push(Step::UnfoldedExists);
            steps.len() - 1
        }
    };

    if let Some(id) = expr.id.as_deref() {
        memo.insert(id, index);
    }
    index
}

/// 把一个分支编译成独立子程序（H 批）。
///
/// 子程序有自己的值表与根索引；`defs` 仍是整棵表达式的定义表 ⇒ 分支里对
/// 外层 `id` 的 `Ref` 照常解析（共享节点在分支内重新编译一次是纯函数，无副作用）。
fn compile_branch<'a>(expr: &'a Expr, defs: &BTreeMap<&'a str, &'a Expr>) -> CompiledExpr {
    let mut steps = Vec::new();
    let mut memo = BTreeMap::new();
    let root = compile_node(expr, defs, &mut memo, &mut steps);
    CompiledExpr {
        steps,
        root,
        requires_replay: expr.requires_replay(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Id;
    use std::cell::Cell;

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    struct ScalarMono {
        time: f64,
        frame: u64,
        calls: Cell<usize>,
    }

    impl ScalarMono {
        fn new(time: f64, frame: u64) -> Self {
            Self {
                time,
                frame,
                calls: Cell::new(0),
            }
        }
    }

    impl Mono for ScalarMono {
        fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
            match (channel.namespace.as_str(), channel.name.as_str()) {
                ("", "time") => Ok(Value::f64(self.time)),
                ("", "frame") => Ok(Value::f64(self.frame as f64)),
                _ => Err(EvalError::MissingChannel {
                    channel: channel.clone(),
                }),
            }
        }

        fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
            self.calls.set(self.calls.get() + 1);
            let num = |index: usize| -> Result<f64, EvalError> {
                args.get(index)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
            };
            match op.name.as_str() {
                "sin" => Ok(Value::f64(num(0)?.sin())),
                "cos" => Ok(Value::f64(num(0)?.cos())),
                "mul" => Ok(Value::f64(num(0)? * num(1)?)),
                "add" => Ok(Value::f64(num(0)? + num(1)?)),
                "scale" => Ok(Value::f64(num(0)? * num(1)?)),
                "sum" => {
                    let mut total = 0.0;
                    for index in 0..args.len() {
                        total += num(index)?;
                    }
                    Ok(Value::f64(total))
                }
                other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
            }
        }
    }

    /// 共享节点：`h1 = sin(t × 1)`，根 = `h1 + 2 × h1`。
    fn shared_tree() -> Expr {
        let h1 = Expr::call(math("sin"), [Expr::call(math("mul"), [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::pure(1.0),
        ])])
        .with_id("h1");
        Expr::call(
            math("add"),
            [
                h1.clone(),
                Expr::call(math("scale"), [Expr::pure(2.0), Expr::reference("h1")]),
            ],
        )
    }

    #[test]
    fn shared_node_is_compiled_and_evaluated_once() {
        let program = CompiledExpr::compile(&shared_tree()).expect("编译成功");
        // 通道 + 纯量 + mul + sin + 纯量 + scale + add = 7 步（h1 只编译一次）。
        assert_eq!(program.step_count(), 7);

        let t = std::f64::consts::FRAC_PI_2;
        let mono = ScalarMono::new(t, 0);
        let value = program.eval(&mono).expect("求值成功");
        let sin = t.sin();
        assert!((value.as_f64().unwrap() - (sin + 2.0 * sin)).abs() < 1e-12);
        // 4 次算子调用（mul / sin / scale / add），共享节点没有重复调用。
        assert_eq!(mono.calls.get(), 4);
    }

    #[test]
    fn evaluation_is_frame_local_and_recomputable() {
        let program = CompiledExpr::compile(&shared_tree()).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let first = program.eval(&mono).unwrap();
        let second = program.eval(&mono).unwrap();
        assert_eq!(first, second, "同一 M ⇒ 同一结果（无隐藏状态）");
        assert_eq!(mono.calls.get(), 8, "每次求值都重算，不跨帧缓存");
    }

    #[test]
    fn frame_channel_comes_from_the_monad() {
        let expr = Expr::call(math("sin"), [Expr::channel(ChannelRef::builtin("frame"))]);
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 30);
        let value = program.eval(&mono).unwrap().as_f64().unwrap();
        assert!((value - 30.0_f64.sin()).abs() < 1e-12);
    }

    #[test]
    fn missing_channel_is_explicit() {
        let expr = Expr::channel(ChannelRef::builtin("event"));
        let program = CompiledExpr::compile(&expr).unwrap();
        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::MissingChannel {
                channel: ChannelRef::builtin("event")
            }
        );
    }

    #[test]
    fn bad_ref_is_rejected_at_compile() {
        let expr = Expr::reference("missing");
        assert_eq!(
            CompiledExpr::compile(&expr).unwrap_err(),
            ExprError::DanglingRef(Id::from("missing"))
        );
    }

    #[test]
    fn unknown_operator_reports_its_source() {
        let expr = Expr::call(OpRef::new("zela.future", "warp"), []);
        let program = CompiledExpr::compile(&expr).unwrap();
        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::Op {
                op: "zela.future::warp".to_string(),
                message: "未知算子 `warp`".to_string(),
            }
        );
    }

    /// **H 批**：`Select` 按 `cond` 取值分支。
    #[test]
    fn select_picks_branch_by_condition() {
        let expr = Expr::select(Expr::pure(true), Expr::pure(1.0), Expr::pure(0.0));
        let program = CompiledExpr::compile(&expr).expect("Select 已实现");
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::f64(1.0)
        );

        let expr = Expr::select(Expr::pure(false), Expr::pure(1.0), Expr::pure(0.0));
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::f64(0.0)
        );
    }

    /// **H 批惰性**：未选中的分支**一次都不求值**（分支里读缺失通道也不报错）。
    #[test]
    fn select_is_lazy_on_the_unselected_branch() {
        let then = Expr::channel(ChannelRef::builtin("event"));
        let r#else = Expr::pure(0.0);
        let expr = Expr::select(Expr::pure(false), then, r#else);
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::f64(0.0),
            "else 分支生效 ⇒ then 里的缺失通道不会被读到"
        );

        // 反向：选中 then（有值）时 else 的缺失通道同样不读。
        let expr = Expr::select(
            Expr::pure(true),
            Expr::pure(1.0),
            Expr::channel(ChannelRef::builtin("event")),
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::f64(1.0)
        );
    }

    /// **H 批**：`cond` 不是 Bool ⇒ 显式报错（不静默当真 / 假）。
    #[test]
    fn select_condition_must_be_bool() {
        let expr = Expr::select(Expr::pure(1.0), Expr::pure(1.0), Expr::pure(0.0));
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::SelectCondition { actual: "scalar" }
        );
    }

    /// **G 批**：`Read` 编译成 `Step::Read`；宿主未实现 `read_object` 时
    /// 默认行为是**显式失败**（`MissingChannel`），不是静默 0。
    #[test]
    fn read_without_host_impl_fails_explicitly() {
        let expr = Expr::read(
            ChannelRef::new("zela.scene", "Circle.radius").with_type(crate::expr::ChannelType::Number),
            7,
        );
        let program = CompiledExpr::compile(&expr).expect("Read 是叶子，编译必然成功");
        assert_eq!(
            program.steps(),
            &[Step::Read {
                channel: ChannelRef::new("zela.scene", "Circle.radius"),
                object: 7,
            }]
        );

        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::MissingChannel {
                channel: ChannelRef::new("zela.scene", "Circle.radius"),
            },
            "默认 read_object ⇒ 显式 MissingChannel"
        );
    }

    /// **阶段 1**：`I64` 不经过 f64（递归索引不能撞刀刃取整）。
    #[test]
    fn i64_stays_integer_and_is_not_silently_cast() {
        let value = Value::i64(7);
        assert_eq!(value.as_i64(), Some(7));
        assert_eq!(value.as_f64(), None, "I64 不得静默转 f64");
        assert_eq!(value.kind(), "i64");
        assert_ne!(
            hash_values(&[Value::i64(7)]),
            hash_values(&[Value::f64(7.0)]),
            "整数与浮点必须是不同的值序列"
        );
    }

    /// **阶段 1**：`Value::Path` 已消失；路径是 `Bytes + zela.Path`。
    #[test]
    fn bytes_carry_a_named_type() {
        let bytes = Value::Bytes(ValueBytes::new("zela.Path", vec![1, 2, 3, 4]));
        assert_eq!(bytes.kind(), "bytes");
        let canonical = bytes.canonical_bytes();
        assert_eq!(canonical[0], 5, "Bytes 的规范标签");
        assert!(canonical.windows("zela.Path".len()).any(|w| w == b"zela.Path"));
    }

    /// **阶段 1 门禁（§0.1）**：`Scalar` 能承载哪些 `ScalarKind` 必须**逐条登记**。
    ///
    /// 三层强制：`lane_width` / `ty_code` 是穷尽 `match`（新增种类不处理 ⇒ **编译失败**）；
    /// 本用例钉住"哪些能作载荷"，改动分类必须同时改这里（**测试失败**）。
    #[test]
    fn scalar_payload_kinds_are_explicitly_classified() {
        use ze_type::ScalarKind::*;
        const ALL: &[ScalarKind] = &[F32, F64, I32, I64, U32, U64, U8, Bool, String];

        let payload: Vec<&str> = ALL
            .iter()
            .filter(|kind| Scalar::lane_width(**kind).is_some())
            .map(|kind| kind_code(*kind))
            .collect();
        let rejected: Vec<&str> = ALL
            .iter()
            .filter(|kind| Scalar::lane_width(**kind).is_none())
            .map(|kind| kind_code(*kind))
            .collect();

        assert_eq!(payload, vec!["f32", "f64", "i32", "i64", "u32", "u64", "u8"]);
        assert_eq!(
            rejected,
            vec!["bool", "string"],
            "Bool 有独立变体，String 是变长（阶段 2 下沉 Bytes）"
        );
    }

    /// **阶段 1**：标量与向量是同一个变体（`ty × dim`），`dim` 上限 4。
    #[test]
    fn scalar_and_vectors_share_one_representation() {
        assert_eq!(Value::f64(1.5).kind(), "scalar");
        assert_eq!(Value::i64(3).kind(), "i64");
        assert_eq!(Value::vec2([1.0, 2.0]).kind(), "vec2");
        assert_eq!(Value::vec3([1.0, 2.0, 3.0]).kind(), "vec3");
        assert_eq!(Value::vec4([1.0, 2.0, 3.0, 4.0]).kind(), "vec4");

        assert_eq!(Value::vec3([1.0, 2.0, 3.0]).as_vec3(), Some([1.0, 2.0, 3.0]));
        assert_eq!(Value::vec2([1.0, 2.0]).as_vec3(), None, "不做隐式升维");
        assert_eq!(Value::f64(1.0).as_vec2(), None, "标量不是向量");
        assert_eq!(Value::vec4([1.0, 2.0, 3.0, 4.0]).as_vec4(), Some([1.0, 2.0, 3.0, 4.0]));
    }

    /// 既有四种组合的规范字节**逐位不变**（值序列哈希是基线的一部分）。
    #[test]
    fn canonical_bytes_keep_the_legacy_tags() {
        assert_eq!(
            Value::f64(1.5).canonical_bytes(),
            [vec![0], 1.5_f64.to_bits().to_le_bytes().to_vec()].concat()
        );
        assert_eq!(
            Value::i64(7).canonical_bytes(),
            [vec![6], 7_i64.to_le_bytes().to_vec()].concat()
        );
        assert_eq!(
            Value::vec2([1.0, 2.0]).canonical_bytes(),
            [
                vec![3],
                1.0_f32.to_le_bytes().to_vec(),
                2.0_f32.to_le_bytes().to_vec()
            ]
            .concat()
        );
        assert_eq!(
            Value::vec3([1.0, 2.0, 3.0]).canonical_bytes(),
            [
                vec![4],
                1.0_f32.to_le_bytes().to_vec(),
                2.0_f32.to_le_bytes().to_vec(),
                3.0_f32.to_le_bytes().to_vec()
            ]
            .concat()
        );
        // 新组合（`Vec4`）走通用标签 7：`ty_code + dim + 分量`。
        let vec4 = Value::vec4([1.0, 2.0, 3.0, 4.0]).canonical_bytes();
        assert_eq!(vec4[0], 7);
        assert_eq!(vec4[1], 0, "F32 的 ty_code");
        assert_eq!(vec4[2], 4, "dim");
    }

    /// `Scalar` 的 serde 往返（缓存可序列化 ⇒ 值必须能过 JSON）。
    #[test]
    fn scalar_round_trips_through_json() {
        for value in [
            Value::f64(-0.0),
            Value::i64(i64::MIN),
            Value::vec2([0.1, -0.2]),
            Value::vec4([f32::MIN, 1.0, 2.0, f32::MAX]),
        ] {
            let json = serde_json::to_string(&value).expect("可序列化");
            let back: Value = serde_json::from_str(&json).expect("可反序列化");
            assert_eq!(back, value, "JSON 往返必须逐位不变：{json}");
        }
    }


    /// **阶段 2**：`Text` 下沉为 `Bytes + zela.Text`，往返逐位不变。
    #[test]
    fn text_sinks_into_named_bytes_without_changing_the_canonical_bytes() {
        let text = "时间 t: π";
        let value = Value::text(text);
        assert_eq!(value.kind(), "text", "诊断词汇仍是 text");
        assert_eq!(value.as_text(), Some(text), "往返逐位不变");
        assert_eq!(
            value.as_bytes().map(|bytes| bytes.ty.as_str()),
            Some(TEXT_TYPE)
        );

        // 规范编码与阶段 1 之前的 `Value::Text` 逐位相同（标签 2 + len + utf8）。
        let mut legacy = vec![2u8];
        legacy.extend_from_slice(&(text.len() as u32).to_le_bytes());
        legacy.extend_from_slice(text.as_bytes());
        assert_eq!(value.canonical_bytes(), legacy);

        // 其它命名类型的字节块仍是 bytes（不静默当文本）。
        let foreign = Value::Bytes(ValueBytes::new("acme.Tensor", vec![0xff]));
        assert_eq!(foreign.kind(), "bytes");
        assert_eq!(foreign.as_text(), None);
    }

    // ─── 阶段 3：NatRec 离散层 ─────────────────────────────────────────

    /// `value(k) = value(k−1) + 2`，`init = 1`，`count = 4` ⇒ `9`。
    fn nat_rec_add_two(count: i64) -> Expr {
        Expr::nat_rec(
            Expr::pure(count),
            Expr::pure(1.0),
            Expr::call(math("add"), [Expr::rec_prev(), Expr::pure(2.0)]),
        )
    }

    #[test]
    fn nat_rec_matches_hand_unrolled_expansion() {
        let expr = nat_rec_add_two(4);
        expr.validate().expect("NatRec 是合法递归结构");
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let lowered = program.eval(&mono).unwrap();
        assert_eq!(lowered, Value::f64(9.0));
        assert_eq!(
            crate::reference::eval_reference(&expr, &mono).unwrap(),
            lowered,
            "参考求值器与 lowered 必须逐位一致"
        );

        // 手写展开：((((1+2)+2)+2)+2)。
        let mut manual = Expr::pure(1.0);
        for _ in 0..4 {
            manual = Expr::call(math("add"), [manual, Expr::pure(2.0)]);
        }
        assert_eq!(
            CompiledExpr::compile(&manual).unwrap().eval(&mono).unwrap(),
            lowered
        );
    }

    /// `RecIndex` 是 `I64`：`value(k) = k` ⇒ `value(count) = count`。
    #[test]
    fn rec_index_is_an_i64_state() {
        let expr = Expr::nat_rec(
            Expr::pure(3_i64),
            Expr::pure(0_i64),
            Expr::rec_index(),
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::i64(3)
        );
    }

    /// 硬约束 6：`count` 不是 `I64` / 为负 ⇒ 显式错误（不取整、不静默 0 轮）。
    #[test]
    fn nat_rec_rejects_non_i64_counts() {
        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(4.0),
            Expr::pure(1.0),
            Expr::rec_prev(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecCount { actual: "scalar" }
        );

        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(-1_i64),
            Expr::pure(1.0),
            Expr::rec_prev(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecCount { actual: "i64" }
        );
    }

    /// 动态护栏：轮数超过库级上限 ⇒ 显式错误（不"跑满上限用最后一次"）。
    #[test]
    fn nat_rec_budget_is_explicit() {
        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(REC_MAX_STEPS + 1),
            Expr::pure(1.0),
            Expr::rec_prev(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecBudget {
                count: REC_MAX_STEPS + 1,
                max_steps: REC_MAX_STEPS,
            }
        );
    }

    /// 状态类型在折叠过程中变化 ⇒ 显式错误（不静默换类型）。
    #[test]
    fn nat_rec_state_type_must_stay_stable() {
        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(1_i64),
            Expr::pure(1.0),
            Expr::rec_index(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecStateType {
                expected: "scalar",
                actual: "i64",
                index: 1,
            }
        );
    }

    /// **合法递归 vs 非法环**：`NatRec` 通过；`Ref` 成环仍然被拒绝。
    #[test]
    fn recursion_is_not_a_ref_cycle() {
        nat_rec_add_two(2).validate().expect("NatRec 合法");

        let a = Expr::call(math("add"), [Expr::reference("b")]).with_id("a");
        let b = Expr::call(math("add"), [Expr::reference("a")]).with_id("b");
        let root = Expr::call(math("add"), [a, b]);
        assert!(matches!(
            root.validate().unwrap_err(),
            ExprError::Cycle(_)
        ));
    }

    /// 递归叶子越界 ⇒ 结构守卫显式拒绝。
    #[test]
    fn recursive_leaves_outside_a_step_are_rejected() {
        let expr = Expr::call(math("add"), [Expr::rec_prev(), Expr::pure(1.0)]);
        assert_eq!(
            expr.validate().unwrap_err(),
            ExprError::OrphanRec("rec_prev")
        );

        let expr = Expr::nat_rec(
            Expr::pure(1_i64),
            Expr::rec_index(),
            Expr::rec_prev(),
        );
        assert_eq!(
            expr.validate().unwrap_err(),
            ExprError::OrphanRec("rec_index")
        );
    }

    /// **Cache 作显式参数**：可序列化、可清空；清空重算逐位相同。
    #[test]
    fn nat_rec_cache_is_serializable_and_replay_is_bit_identical() {
        let program = CompiledExpr::compile(&nat_rec_add_two(5)).unwrap();
        let mono = ScalarMono::new(0.0, 0);

        let mut cache = None;
        let first = program.eval_cached(&mono, &mut cache).unwrap();
        assert_eq!(first, Value::f64(11.0));
        let cache = cache.expect("根级 NatRec 必须产出缓存");
        assert_eq!(cache.count, 5);

        // 序列化 → 反序列化 → 复用。
        let json = serde_json::to_string(&cache).expect("缓存可序列化");
        let mut restored: Option<RecCache> = serde_json::from_str(&json).unwrap();
        assert_eq!(
            program.eval_cached(&mono, &mut restored).unwrap(),
            first,
            "复用缓存必须逐位相同"
        );

        // 清空缓存 ⇒ 从 init 重放，仍逐位相同。
        let mut cleared = None;
        assert_eq!(program.eval_cached(&mono, &mut cleared).unwrap(), first);

        // 部分缓存（count = 2）⇒ 从该状态续算到 5，与全量重放一致。
        let partial_program = CompiledExpr::compile(&nat_rec_add_two(2)).unwrap();
        let mut partial = None;
        partial_program.eval_cached(&mono, &mut partial).unwrap();
        assert_eq!(partial.as_ref().unwrap().count, 2);
        let resumed = program.eval_cached(&mono, &mut partial).unwrap();
        assert_eq!(resumed, first, "续算必须与全量重放逐位相同");
    }

    /// 多个 `NatRec` 时缓存只属于**第一个被求值**的那个（不串状态）。
    #[test]
    fn nat_rec_cache_is_owned_by_exactly_one_fold() {
        let expr = Expr::call(
            math("add"),
            [nat_rec_add_two(2), nat_rec_add_two(3)],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let mut cache = None;
        let value = program.eval_cached(&mono, &mut cache).unwrap();
        assert_eq!(value, Value::f64(12.0), "5 + 7");
        let cache = cache.expect("第一个 NatRec 产出缓存");
        assert_eq!(cache.count, 2, "缓存属于第一个 fold");
        assert_eq!(
            program.eval_cached(&mono, &mut Some(cache)).unwrap(),
            value
        );
    }

    /// **G 批**：宿主覆写 `read_object` 后，`Read` 返回注入值。
    #[test]
    fn read_returns_injected_value_when_host_overrides() {
        struct InjectedRead {
            radius: f64,
        }
        impl Mono for InjectedRead {
            fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
                Err(EvalError::MissingChannel {
                    channel: channel.clone(),
                })
            }
            fn call(&self, op: &OpRef, _args: &[Value]) -> Result<Value, EvalError> {
                // 本测试只求值 Read 叶子，算子不会被调用；显式失败兜底。
                Err(EvalError::op(op, "测试单子不实现 call".to_string()))
            }
            fn read_object(&self, channel: &ChannelRef, object: u64) -> Result<Value, EvalError> {
                if channel.name == "Circle.radius" {
                    Ok(Value::f64(self.radius))
                } else {
                    Err(EvalError::MissingComponent {
                        object,
                        channel: channel.clone(),
                    })
                }
            }
        }

        let expr = Expr::read(ChannelRef::new("zela.scene", "Circle.radius"), 7);
        let program = CompiledExpr::compile(&expr).unwrap();
        let value = program.eval(&InjectedRead { radius: 2.5 }).unwrap();
        assert!((value.as_f64().unwrap() - 2.5).abs() < 1e-12);

        // 地址匹配但对象上无组件 ⇒ `MissingComponent` 变体存在且显式。
        let missing = Expr::read(ChannelRef::new("zela.scene", "Dash.period"), 7);
        let program = CompiledExpr::compile(&missing).unwrap();
        let err = program.eval(&InjectedRead { radius: 0.0 }).unwrap_err();
        assert!(matches!(
            err,
            EvalError::MissingComponent { object, .. } if object == 7
        ));
    }
}
