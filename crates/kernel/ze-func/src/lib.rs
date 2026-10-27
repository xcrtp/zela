//! ze-func：**基础函子库**（namespace `zela.math`）。
//!
//! # 分层
//!
//! ```text
//! ze-comb   表达式内核：Expr / Value / Mono / Pass / 参考求值器（领域无关）
//!    ▲
//! ze-func   本 crate：算子语义 + OpDef 元数据（pointwise / targets / revision）
//!    ▲
//! ze-pod-func  三语言接线 + 装配期类型推导（infer）
//! ```
//!
//! 本 crate 只依赖 `ze-comb`：算子的**身份**与**语义**同源，不引入 pod 机制、
//! 也不依赖任何宿主 crate。这样表达式内核可以在没有 pod / 场景 / 渲染的情况下
//! 独立测试（计划 §阶段 0）。
//!
//! # 当前边界
//!
//! - 只做标量 / 向量算子（`f64` / `Vec2` / `Vec3`）；路径等私有类型走
//!   `ze-comb::Value::Bytes` + 命名类型（阶段 1）。
//! - 全部现有算子声明为 `pointwise`（连续层）；离散层（`NatRec`）在阶段 3 引入。

pub mod layout;
pub mod op;
pub mod ops;
pub mod simplify;

pub use layout::{
    ENDIAN, Endian, FieldLayout, Layout, LayoutError, PATH_TYPE, Record, Scalar, Transcode,
    TypeRegistry, decode_path, decode_record, encode_path, encode_record, negotiate_bytes,
};
pub use op::{
    DEFS as OP_DEFS, LIBRARY_NAMESPACE, OpDef, Target, is_pointwise, op_def, op_revision, targets,
};
pub use ops::{OPS, eval, is_known};
pub use simplify::{SIMPLIFY_REC_LIMIT, Simplify, simplify};
