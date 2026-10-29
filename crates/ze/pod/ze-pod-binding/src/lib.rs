//! ze-pod-binding：**语言无关**的绑定驱动。
//!
//! ```text
//! capability 的 calls()  ──►  BindingPlan（函数 + 可达类型）
//!                                   │
//!                    ┌──────────────┴──────────────┐
//!                    ▼                             ▼
//!              Lua 适配器                    Python 适配器
//!              ValueCodec = LuaValue           ValueCodec = PyObject
//! ```
//!
//! 三层职责：
//!
//! | 模块 | 内容 | 依赖 |
//! |------|------|------|
//! | [`plan`] | `BindingPlan` / `component_ctor` / 类型收集 | `ze-pod-core` |
//! | [`codec`] | `ValueCodec` + 通用调用编排 [`invoke`] | `ze-pod-core` |
//! | [`docs`] | JSON 文档（唯一来源） | `ze-pod-core` |
//!
//! **本 crate 不出现任何具体语言类型**（不含语言运行时依赖）；
//! `cargo tree -p ze-pod-binding` 只有 `ze-pod-core` + 自身，可进 wasm gate。
#![forbid(unsafe_code)]

pub mod codec;
pub mod defaults;
pub mod docs;
pub mod plan;

pub use codec::{ValueCodec, invoke};
pub use defaults::{field_default, field_default_literal};
pub use docs::{
    CapabilityDocs, DOCS_SCHEMA_VERSION, capabilities_json, component_ctor_of, has_ops, op_by_name,
    op_by_spec, reachable_types,
};
pub use plan::{BindingPlan, collect_types, short_name};

/// 通道类型（`FieldSpec.ty`）→ `zela.math.ChannelType` 的**变体名**。
///
/// 第 2 批：类型随通道常量过边界，三个语言绑定都用这个函数把 `TypeSpec` 变成
/// 结构化枚举值。F 批的字段类型是封闭集（标量 / `Vec2` / `Vec3` / `bool`）；
/// 其它一律 `Unknown` —— **不静默当 `Number`**（否则脚本边界又出现一次隐式退化，
/// 装配期 `infer` 会把 `Unknown` 显式报成"缺少类型信息"）。
pub fn channel_type_variant(spec: &ze_pod_core::TypeSpec) -> &'static str {
    use ze_pod_core::{ScalarKind, TypeSpec};
    match spec {
        TypeSpec::Scalar(ScalarKind::Bool) => "Bool",
        TypeSpec::Scalar(_) => "Number",
        TypeSpec::Struct {
            name: "zela.Vec2", ..
        } => "Vec2",
        TypeSpec::Struct {
            name: "zela.Vec3", ..
        } => "Vec3",
        _ => "Unknown",
    }
}
