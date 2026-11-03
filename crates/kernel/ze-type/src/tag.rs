//! Type tags and socket-level type compatibility（阶段 4：收敛到 `ze-type`）。
//!
//! 图把值存成 `Box<dyn Any + Send>`（运行期擦除），每个 socket 携带一个
//! [`TypeTag`] 描述它接受/产出的逻辑类型，[`TypeCast`] 决定两个 tag 能否相连。
//!
//! 这里只依赖 `core` + `alloc`：`ze-type` 保持 `no_std` 可用。

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use core::any::Any;
use core::fmt;

use serde::{Deserialize, Serialize};

/// Logical type identifier carried by a socket.
pub trait TypeTag: fmt::Debug + Send + Sync + 'static {
    /// Clone onto the heap (object-safe clone).
    fn clone_box(&self) -> Box<dyn TypeTag>;
    /// Object-safe equality.
    fn dyn_eq(&self, other: &dyn TypeTag) -> bool;
    /// Stable human-readable name (used for diagnostics & serde tagging).
    fn type_name(&self) -> &'static str;
    /// Downcast access for casters that want the concrete tag.
    fn as_any(&self) -> &dyn Any;
}

impl Clone for Box<dyn TypeTag> {
    fn clone(&self) -> Self {
        self.as_ref().clone_box()
    }
}

impl PartialEq for Box<dyn TypeTag> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref().dyn_eq(other.as_ref())
    }
}

impl Eq for Box<dyn TypeTag> {}

/// A trivial string-named type tag for tests and simple domains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedTypeTag {
    name: &'static str,
}

impl NamedTypeTag {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl TypeTag for NamedTypeTag {
    fn clone_box(&self) -> Box<dyn TypeTag> {
        Box::new(self.clone())
    }
    fn dyn_eq(&self, other: &dyn TypeTag) -> bool {
        match other.as_any().downcast_ref::<Self>() {
            Some(other) => self == other,
            None => false,
        }
    }
    fn type_name(&self) -> &'static str {
        self.name
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Error raised when a value cannot be cast between two type tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastError {
    Incompatible { from: String, to: String },
    ValueTypeMismatch { tag: String },
}

impl fmt::Display for CastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incompatible { from, to } => {
                write!(f, "incompatible types: {from} -> {to}")
            }
            Self::ValueTypeMismatch { tag } => {
                write!(f, "value does not match tag {tag}")
            }
        }
    }
}

impl core::error::Error for CastError {}

/// Decides type compatibility and performs implicit conversion between two
/// [`TypeTag`]s.
pub trait TypeCast: Send + Sync + 'static {
    /// Can a value of `from` flow into a socket expecting `to`?
    fn is_compatible(&self, from: &dyn TypeTag, to: &dyn TypeTag) -> bool;
    /// Convert `value` (tagged `from`) into a value tagged `to`.
    fn try_cast(
        &self,
        value: Box<dyn Any + Send>,
        from: &dyn TypeTag,
        to: &dyn TypeTag,
    ) -> Result<Box<dyn Any + Send>, CastError>;
}

/// Default caster: identity only.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultTypeCast;

impl TypeCast for DefaultTypeCast {
    fn is_compatible(&self, from: &dyn TypeTag, to: &dyn TypeTag) -> bool {
        from.dyn_eq(to)
    }
    fn try_cast(
        &self,
        value: Box<dyn Any + Send>,
        from: &dyn TypeTag,
        to: &dyn TypeTag,
    ) -> Result<Box<dyn Any + Send>, CastError> {
        if from.dyn_eq(to) {
            Ok(value)
        } else {
            Err(CastError::Incompatible {
                from: from.type_name().to_string(),
                to: to.type_name().to_string(),
            })
        }
    }
}

/// 诊断辅助：两个 tag 的名字（`from -> to`）。
pub fn describe_cast(from: &dyn TypeTag, to: &dyn TypeTag) -> String {
    format!("{} -> {}", from.type_name(), to.type_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_tags_compare_and_clone() {
        let a: Box<dyn TypeTag> = Box::new(NamedTypeTag::new("f32"));
        let b = a.clone();
        let other: Box<dyn TypeTag> = Box::new(NamedTypeTag::new("i32"));
        assert!(a.dyn_eq(b.as_ref()));
        assert!(!a.dyn_eq(other.as_ref()));
        assert_eq!(a.type_name(), "f32");
    }

    #[test]
    fn default_cast_is_identity_only() {
        let cast = DefaultTypeCast;
        let f32_tag = NamedTypeTag::new("f32");
        let f64_tag = NamedTypeTag::new("f64");
        assert!(cast.is_compatible(&f32_tag, &f32_tag));
        assert!(!cast.is_compatible(&f32_tag, &f64_tag));
    }
}
