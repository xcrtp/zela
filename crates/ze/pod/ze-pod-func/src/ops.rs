//! `zela.math` 算子的**接线入口**（语义实现已迁到基础函子库 `ze-func`）。
//!
//! 本模块只保留"属于本 pod 的那一点事"：算子是否属于本库（namespace 校验）。
//! 语义实现、`OpDef` 元数据（`since/changed/pointwise/targets`）都在 `ze-func`，
//! 保证表达式层与求值层不会各说各话。

use ze_comb::OpRef;

pub use ze_func::{OPS, eval};

/// 本函子库的 namespace（与 `ze-func` 同一份常量）。
pub const LIBRARY_NAMESPACE: &str = ze_func::LIBRARY_NAMESPACE;

/// `op` 是否属于本库（namespace 必须一致，名字在 [`OPS`] 里）。
pub fn is_known(op: &OpRef) -> bool {
    op.namespace == LIBRARY_NAMESPACE && OPS.contains(&op.name.as_str())
}
