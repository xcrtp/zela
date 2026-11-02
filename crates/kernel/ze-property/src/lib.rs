//! `ze-property` — the kernel-level property model: schema + value + constraints.
//!
//! A **property** is "one tunable parameter" described without any UI or host
//! dependency: its label, kind, value and hard bounds. It is the vocabulary
//! shared by every consumer of a parameter:
//!
//!   * the node stack (`ze-node-schema` implements [`NodeSpec`](https://docs.rs/zela)
//!     on top of [`Property`]);
//!   * the scene / script assembly layer (a parameter registry);
//!   * a UI panel, reading the same [`Property`] to pick a widget;
//!   * a script or headless evaluator, reading/writing values with no UI at all.
//!
//! Provides:
//!   * [`Property`] / [`PropertyValue`] — a serializable, UI-neutral vocabulary
//!     of parameter kinds (scalar, vec, color, enum, range, function, dynamic,
//!     list, …);
//!   * [`DynamicObject`] — a runtime-extensible, ordered bag of named
//!     properties (the "user can add fields without recompiling" half).
//!
//! Hard constraint (CA plan §5.1): this crate depends only on `serde` +
//! `thiserror` + `std`. It must **not** depend on `egui`, `glam`, any node/graph
//! crate, or any `zela-*` crate — that is what lets the parameter panel exist
//! before the node stack is decoupled (R-08).

#![forbid(unsafe_code)]

pub mod dynamic;
pub mod error;
pub mod property;
pub mod registry;

pub use dynamic::{DynamicField, DynamicObject};
pub use error::PropertyError;
pub use property::{
    BoolConfig, ColorConfig, DoubleConfig, DynamicConfig, EnumConfig, FunctionConfig,
    FunctionHandle, IntegerConfig, ListConfig, Property, PropertyValue, RangeConfig, ScalarConfig,
    StringConfig, StringKind, VEC_MAX_COMPONENTS, VecNConfig,
};
pub use registry::{ParameterEntry, ParameterRegistry};
