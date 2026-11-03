//! Type tags and socket-level type compatibility.
//!
//! **阶段 4 起**：实现迁至 [`ze_type::tag`]（kernel 层，`no_std` 可用）；
//! 本模块只 re-export，保持 `ze_node_core::TypeTag` 等路径不变。

pub use ze_type::tag::{
    CastError, DefaultTypeCast, NamedTypeTag, TypeCast, TypeTag, describe_cast,
};
