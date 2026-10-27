//! 类型描述（IDL schema）：**实现已迁至 `ze-type`**，本模块只做 re-export +
//! 保留 Pod 绑定入口 [`ErasedFn`]。
//!
//! # 为什么迁移
//!
//! 十一个类型（`ScalarKind` / `TypeSpec` / `Field` / `Variant` / `Kind` / `OpSig` /
//! `RetType` / `OpDef` / `ParamSpec` / `FnIo`）是**纯代数**，与 Pod 机制无关。
//! 它们现在住在 `crates/kernel/ze-type`，可被 build script / wasm / 代码生成后端
//! 单独复用，而不必拉进整个 Pod 层。
//!
//! `ErasedFn` 是唯一留在本模块的类型：它持有 `fn(&dyn Pod, &[PodValue])`，
//! 是**运行时绑定入口**，不属于代数层。
//!
//! # 与 serde 的分工
//!
//! serde 是序列化协议（推 / 拉），本模块是**类型反射**（生成文档与绑定）。
//! 两者数据模型同构，但用途不同，故不复用。

pub use ze_type::{
    Field, FnIo, Kind, OpDef, OpSig, ParamSpec, RetType, ScalarKind, Target, TypeSpec, Variant,
    HANDLE_SPEC, UNIT_SPEC,
};

use crate::capability::FnSig;
use crate::error::PodError;
use crate::value::PodValue;

/// **绑定用的类型擦除入口**：由 `#[capability(bindings)]` 为每个字段生成。
///
/// `call` 在装配期按完整函数名解析出 typed 裸 fn 指针，再做
/// `PodValue ↔ 原生值` 转换 —— **只在跨语言边界**，不进热路径。
#[derive(Clone, Copy)]
pub struct ErasedFn {
    /// 与 [`crate::Capability::signatures`] 中同名同序的那一条。
    pub signature: FnSig,
    /// 形参名 + 结构类型。
    pub io: FnIo,
    /// 类型擦除调用：参数与返回值都是 [`PodValue`]。
    pub call: fn(&dyn crate::Pod, &[PodValue]) -> Result<PodValue, PodError>,
}

impl std::fmt::Debug for ErasedFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErasedFn")
            .field("signature", &self.signature)
            .field("io", &self.io)
            .finish_non_exhaustive()
    }
}
