//! ze-type：**纯代数类型层**。
//!
//! 本 crate 只回答三个问题，且只回答一次：
//!
//! 1. 一个类型长什么样（[`ty::TypeSpec`] / [`layout::Layout`]）；
//! 2. 两个类型能否合一、谁是谁的子类型（[`unify`]）；
//! 3. 两个类型描述冲突时，诊断怎么说（[`conflict::ReflectError`]）。
//!
//! # 为什么独立成 crate
//!
//! 代码生成后端只想算个布局，不该被迫拉进 arena（见计划 §2）。
//! 因此本 crate **零运行时状态**：没有 interning、没有注册表、没有缓存；
//! 所有 `alloc` 只用于诊断文本与实例列表。
//!
//! # 快慢两路径的位置
//!
//! `TypeSpec`（结构比对）是**慢形式**；解析后的 `TypeId` / 符号槽位属于
//! `ze-reflect`。本 crate 只负责慢形式的语义，不负责解析缓存。

#![no_std]

extern crate alloc;

pub mod conflict;
// `gen` 在 edition 2024 是保留字：模块用 raw identifier 声明。
pub mod r#gen;
pub mod layout;
pub mod tag;
pub mod ty;
pub mod unify;

pub use conflict::{Blame, NameKind, ReflectError};
pub use r#gen::{MonoInstance, MAX_INSTANCES};
pub use tag::{CastError, DefaultTypeCast, NamedTypeTag, TypeCast, TypeTag};
pub use layout::{
    ENDIAN, Endian, FieldLayout, FieldTy, Layout, LayoutError, Record, Scalar, Transcode,
    VEC2_FIELDS, VEC3_FIELDS,
};
pub use ty::{
    Field, FnIo, HANDLE_SPEC, Kind, OpDef, OpSig, ParamSpec, RetType, ScalarKind, Target, TypeSpec,
    UNIT_SPEC, VEC2_SPEC, VEC3_SPEC, Variant,
};
