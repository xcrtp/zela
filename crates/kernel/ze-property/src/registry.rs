//! [`ParameterRegistry`] — the enumerable side table of tunable parameters.
//!
//! A registry maps a **parameter address** (a stable string key such as
//! `curve:42/base`) to a [`Property`] (label + kind + value + bounds). Because
//! the map is keyed and ordered, a consumer can *ask what parameters exist* and
//! let a UI generate controls — no hand-written widget ids, no `match` on the
//! concrete driver type.
//!
//! Design constraints (CA plan §5.2):
//!   * `BTreeMap` only — iteration is **ascending by address**, so generated
//!     widget order is deterministic (a `HashMap` would drift with the seed);
//!   * assembled once, read-only at runtime (derived data, never in the payload);
//!   * values stay [`Property`]s, so writes are validated by the same
//!     `set_value` used everywhere else in the property model.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::PropertyError;
use crate::property::{Property, PropertyValue};

/// One registry row: the address and its current schema + value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterEntry<'a> {
    pub address: &'a str,
    pub property: &'a Property,
}

/// An ordered `address → Property` table. Ascending address order is part of
/// the contract (it is what makes generated widget ids stable).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ParameterRegistry {
    entries: BTreeMap<String, Property>,
}

impl ParameterRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of registered parameters.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry has no parameters.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Register a parameter. A duplicate address is an error (it would make the
    /// widget id assignment ambiguous).
    pub fn insert(
        &mut self,
        address: impl Into<String>,
        property: Property,
    ) -> Result<(), PropertyError> {
        let address = address.into();
        if self.entries.contains_key(&address) {
            return Err(PropertyError::DuplicateField(address));
        }
        self.entries.insert(address, property);
        Ok(())
    }

    /// Look up a parameter by address.
    pub fn get(&self, address: &str) -> Option<&Property> {
        self.entries.get(address)
    }

    /// Mutably look up a parameter by address.
    pub fn get_mut(&mut self, address: &str) -> Option<&mut Property> {
        self.entries.get_mut(address)
    }

    /// Type-checked value write (delegates to [`Property::set_value`]).
    pub fn set_value(
        &mut self,
        address: &str,
        value: PropertyValue,
    ) -> Result<(), PropertyError> {
        let property = self
            .entries
            .get_mut(address)
            .ok_or_else(|| PropertyError::UnknownProperty(address.to_string()))?;
        property.set_value(value)
    }

    /// Current value of a parameter.
    pub fn value(&self, address: &str) -> Option<PropertyValue> {
        self.entries.get(address).map(Property::value)
    }

    /// Iterate rows in ascending address order.
    pub fn iter(&self) -> impl Iterator<Item = ParameterEntry<'_>> {
        self.entries
            .iter()
            .map(|(address, property)| ParameterEntry {
                address: address.as_str(),
                property,
            })
    }

    /// Ascending address list.
    pub fn addresses(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Position of `address` in ascending order (the stable widget index).
    pub fn index_of(&self, address: &str) -> Option<usize> {
        self.entries.keys().position(|key| key == address)
    }

    /// Row at an ascending index (inverse of [`index_of`](Self::index_of)).
    pub fn at(&self, index: usize) -> Option<ParameterEntry<'_>> {
        self.entries
            .iter()
            .nth(index)
            .map(|(address, property)| ParameterEntry {
                address: address.as_str(),
                property,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::ScalarConfig;

    fn scalar(label: &str, value: f32, min: f32, max: f32) -> Property {
        Property::Scalar(ScalarConfig {
            label: label.into(),
            value,
            min,
            max,
            step: Some(0.1),
            unit: None,
        })
    }

    #[test]
    fn iteration_is_ascending_by_address() {
        let mut registry = ParameterRegistry::new();
        registry
            .insert("curve:2/damp", scalar("damp", 0.97, 0.0, 1.0))
            .unwrap();
        registry
            .insert("curve:1/stiff", scalar("stiff", 0.02, 0.0, 1.0))
            .unwrap();
        registry
            .insert("curve:10/base", scalar("base", 1.0, 0.0, 3.0))
            .unwrap();
        let addresses: Vec<&str> = registry.addresses().collect();
        assert_eq!(
            addresses,
            vec!["curve:1/stiff", "curve:10/base", "curve:2/damp"]
        );
        // index ↔ address round-trip (stable widget ids rely on it).
        for (index, address) in addresses.iter().enumerate() {
            assert_eq!(registry.index_of(address), Some(index));
            assert_eq!(registry.at(index).unwrap().address, *address);
        }
    }

    #[test]
    fn duplicate_address_is_rejected() {
        let mut registry = ParameterRegistry::new();
        registry
            .insert("a", scalar("a", 0.0, 0.0, 1.0))
            .unwrap();
        assert!(matches!(
            registry.insert("a", scalar("a", 0.5, 0.0, 1.0)),
            Err(PropertyError::DuplicateField(_))
        ));
    }

    #[test]
    fn set_value_is_type_checked_and_clamped() {
        let mut registry = ParameterRegistry::new();
        registry
            .insert("a", scalar("a", 0.0, 0.0, 1.0))
            .unwrap();
        assert!(registry.set_value("a", PropertyValue::Bool(true)).is_err());
        registry
            .set_value("a", PropertyValue::Scalar(9.0))
            .unwrap();
        assert_eq!(registry.value("a"), Some(PropertyValue::Scalar(1.0)));
        assert!(registry.set_value("missing", PropertyValue::Scalar(0.0)).is_err());
    }

    #[test]
    fn serde_round_trip() {
        let mut registry = ParameterRegistry::new();
        registry
            .insert("a", scalar("a", 0.25, 0.0, 1.0))
            .unwrap();
        let json = serde_json::to_string(&registry).unwrap();
        let back: ParameterRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(registry, back);
    }
}
