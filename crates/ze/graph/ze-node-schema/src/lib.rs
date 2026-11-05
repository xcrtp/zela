//! `ze-node-schema` — the property model layer for zela's node graph.
//!
//! Sits above [`ze_node_core`] (the pure node/graph kernel) and below any UI
//! layer. Provides:
//!   * [`Property`] / [`PropertyValue`] — a serializable, UI-neutral vocabulary
//!     of parameter kinds (scalar, vec, color, enum, range, function, dynamic,
//!     list, …). **Defined in the kernel crate [`ze_property`]** (CA-a) and
//!     re-exported here so the historical `ze_node_schema::Property` path keeps
//!     working;
//!   * [`NodeSpec`] — the declarative property interface a node implements on
//!     top of [`ze_node_core::Node`];
//!   * [`NodeRegistry`] — a type-erased catalogue that can spawn node instances
//!     and reach their `NodeSpec` impl from a `&dyn Node` without any caller
//!     downcast (the bridge over Rust's missing trait-upcasting);
//!   * [`DynamicObject`] — a runtime-extensible bag of named properties so
//!     users can add fields without recompiling.
//!
//! Hard constraint (plan §1): this crate depends only on `ze_node_core`,
//! `ze_property` + `serde`. It must **not** depend on `egui`, `zela-node-editor`,
//! `zela-node-egui(-ui)` or any project-internal crate other than `zela-node`.
//! That keeps the "backend-swappable / WASM / headless" architecture promise.

#![forbid(unsafe_code)]

pub mod dynamic_node;
pub mod registry;
pub mod spec;

// The property vocabulary itself lives in the kernel now (CA-a). Re-export the
// **modules** as well as the items: `ze_node_schema::property::Property` and
// `crate::dynamic::DynamicObject` paths keep resolving for existing callers.
pub use dynamic_node::{DynamicNode, DynamicNodeBuilder};
pub use registry::{NodeRegistry, RegistryCategory, RegistryNodeInfo};
pub use spec::{NodeSpec, property_schema, property_value};
pub use ze_property::{
    BoolConfig, ColorConfig, DoubleConfig, DynamicConfig, DynamicField, DynamicObject, EnumConfig,
    FunctionConfig, FunctionHandle, IntegerConfig, ListConfig, ParameterEntry, ParameterRegistry,
    Property, PropertyError, PropertyValue, RangeConfig, ScalarConfig, StringConfig, StringKind,
    VEC_MAX_COMPONENTS, VecNConfig,
};
pub use ze_property::{dynamic, error, property};

/// Re-export of the `#[derive(NodeSpec)]` proc-macro.
///
/// Downstream crates write `use ze_node_schema::NodeSpec;` to get the trait, and
/// `#[derive(NodeSpec)]` resolves to this proc-macro via the trait's name (the
/// derive is invoked in attribute position, which does not conflict with the
/// trait re-export in type position).
pub use ze_node_schema_macros::NodeSpec;
