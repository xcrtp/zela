//! **符号表 = handle arena**（计划 §1）。
//!
//! 句柄是运行时对象的符号；符号是编译期对象的句柄 —— 同一个数据结构的两面。
//!
//! 慢路径把名字解析一次（装配期），快路径只做 `SymbolId → 槽位 → &Entry`。
//! 现在 `TARGETS.iter().position(..)` / `OPS.iter().find(..)` **每次都扫**，
//! 改成整数槽位本身就是性能收益，不只是整洁。

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use ze_type::{Blame, NameKind, ReflectError};

use crate::handle::{HandleCore, HandleError};

/// 符号表的 owner（与句柄机制同形，`SymbolId` 也是 generation 感知句柄）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolSpace;

impl fmt::Display for SymbolSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "symbols")
    }
}

/// 快路径符号：整数槽位 + generation。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId {
    slot: u32,
    generation: u32,
}

impl SymbolId {
    pub const fn new(slot: u32, generation: u32) -> Self {
        Self { slot, generation }
    }

    pub const fn slot(self) -> u32 {
        self.slot
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl HandleCore for SymbolId {
    type Owner = SymbolSpace;

    fn owner(&self) -> Self::Owner {
        SymbolSpace
    }

    fn slot(&self) -> u64 {
        u64::from(self.slot)
    }

    fn generation(&self) -> u32 {
        self.generation
    }
}

struct Slot<K, E> {
    generation: u32,
    key: K,
    value: Option<E>,
}

/// 名字 → 槽位 → 值。解析一次，之后是整数索引。
///
/// `K` 是键类型：`&'static str`（算子表）/ `String`（第三方注册）/ `QualifiedName`
/// （Pod 函数集合）。键只要求 `Eq + Hash + Clone + Display` —— 机制统一，键自由。
pub struct SymbolTable<K, E> {
    slots: Vec<Slot<K, E>>,
    index: HashMap<K, SymbolId>,
    free: Vec<u32>,
}

impl<K, E> Default for SymbolTable<K, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, E> SymbolTable<K, E> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            index: HashMap::new(),
            free: Vec::new(),
        }
    }

    /// 快路径：整数槽位 → 值（generation 不匹配 ⇒ 显式错误）。
    pub fn get(&self, id: SymbolId) -> Result<&E, HandleError> {
        let entry = self
            .slots
            .get(id.slot as usize)
            .filter(|entry| entry.generation == id.generation)
            .ok_or(HandleError::Stale {
                slot: u64::from(id.slot),
                expected: id.generation,
                actual: self
                    .slots
                    .get(id.slot as usize)
                    .map(|entry| entry.generation)
                    .unwrap_or(u32::MAX),
            })?;
        entry.value.as_ref().ok_or(HandleError::Released {
            slot: u64::from(id.slot),
        })
    }

    /// 已登记的名字数量。
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// 全部 `(键, 值)`（文档 / 对拍用）。
    pub fn entries(&self) -> impl Iterator<Item = (&K, &E)> {
        self.slots
            .iter()
            .filter_map(|entry| entry.value.as_ref().map(|value| (&entry.key, value)))
    }
}

impl<K, E> SymbolTable<K, E>
where
    K: Eq + Hash,
{
    /// 装配期解析：名字 → `(SymbolId, &E)`（`HashMap` 查找，O(1)）。
    ///
    /// `Q` 允许用 `&str` 查 `String` 键、用 `&QualifiedName` 查同类型键。
    pub fn resolve<Q>(&self, key: &Q) -> Option<(SymbolId, &E)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let id = *self.index.get(key)?;
        let value = self.get(id).ok()?;
        Some((id, value))
    }

    /// 释放一个符号（槽位复用会递增 generation，旧 `SymbolId` 随即失效）。
    pub fn release(&mut self, id: SymbolId) -> Result<E, HandleError> {
        let entry = self
            .slots
            .get_mut(id.slot as usize)
            .filter(|entry| entry.generation == id.generation)
            .ok_or(HandleError::Released {
                slot: u64::from(id.slot),
            })?;
        let value = entry.value.take().ok_or(HandleError::Released {
            slot: u64::from(id.slot),
        })?;
        self.index.remove(&entry.key);
        self.free.push(id.slot);
        Ok(value)
    }
}

impl<K, E> SymbolTable<K, E>
where
    K: Eq + Hash + Clone + fmt::Display,
{
    /// 解析并登记一个名字。重名 = **显式冲突**（带污染源）。
    pub fn intern(&mut self, key: K, value: E, origin: Blame) -> Result<SymbolId, ReflectError> {
        if self.index.contains_key(&key) {
            return Err(ReflectError::duplicate(
                NameKind::Symbol,
                key.to_string(),
                origin,
            ));
        }
        let id = if let Some(slot) = self.free.pop() {
            let entry = &mut self.slots[slot as usize];
            entry.generation = entry.generation.wrapping_add(1);
            entry.key = key.clone();
            entry.value = Some(value);
            SymbolId::new(slot, entry.generation)
        } else {
            let slot = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                key: key.clone(),
                value: Some(value),
            });
            SymbolId::new(slot, 0)
        };
        self.index.insert(key, id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_resolves_in_constant_time_and_rejects_duplicates() {
        let mut table = SymbolTable::new();
        let sin = table
            .intern("zela.math::sin", 1u32, Blame::Assembly)
            .unwrap();
        let cos = table
            .intern("zela.math::cos", 2u32, Blame::Assembly)
            .unwrap();

        assert_eq!(table.get(sin).unwrap(), &1);
        assert_eq!(table.get(cos).unwrap(), &2);
        assert_eq!(table.resolve("zela.math::sin").map(|(_, v)| *v), Some(1));

        let error = table
            .intern("zela.math::sin", 3u32, Blame::pod("expr"))
            .unwrap_err();
        assert_eq!(error.name(), Some("zela.math::sin"));
        assert_eq!(error.origin(), Blame::pod("expr"));
    }

    #[test]
    fn released_symbols_go_stale_not_wrong() {
        let mut table = SymbolTable::<&'static str, u32>::new();
        let id = table.intern("resolved", 7u32, Blame::Assembly).unwrap();
        assert_eq!(table.release(id).unwrap(), 7);

        let replacement = table.intern("resolved", 8u32, Blame::Assembly).unwrap();
        assert_eq!(replacement.slot(), id.slot());
        assert_ne!(replacement.generation(), id.generation());

        assert!(matches!(table.get(id), Err(HandleError::Stale { .. })));
        assert_eq!(table.get(replacement).unwrap(), &8);
    }

    #[test]
    fn non_str_keys_use_the_same_mechanism() {
        let mut table = SymbolTable::<u32, &'static str>::new();
        let id = table.intern(7, "edge", Blame::Assembly).unwrap();
        assert_eq!(table.resolve(&7).map(|(_, v)| *v), Some("edge"));
        assert!(table.resolve(&8).is_none());
        assert_eq!(table.get(id).unwrap(), &"edge");
    }
}
