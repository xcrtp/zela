//! **统一句柄机制**：generation 感知的槽位表 + 解析协议。
//!
//! # 统一的是机制，不是类型
//!
//! `hecs::Entity` 改不了、`ObjectId` 已打包进 payload —— 格式钉死的类型不能替换成
//! 同一个 Rust 类型。统一的是**机制**：一个 [`HandleCore`] trait（owner + slot +
//! generation + 校验）与一张 [`SlotTable`]；各领域类型实现 trait 即可。
//!
//! # 为什么 generation 必须补
//!
//! 没有 generation 的句柄在槽位复用后会**读到别人的数据**。`ze-pod-core` 的
//! `Handle` 早就解决了；`FunctionId` 却退化成线性扫描的裸 `u64`（计划 §0.2）。
//! 本模块把答案收拢到一处。

use std::fmt;

use ze_type::{Blame, NameKind, ReflectError};

/// generation 感知句柄的最小协议。
pub trait HandleCore {
    /// owner 身份（Pod / 场景 / 渲染后端 …）。
    type Owner: Copy + Eq + fmt::Display;

    fn owner(&self) -> Self::Owner;
    fn slot(&self) -> u64;
    fn generation(&self) -> u32;

    /// 校验 owner 与 generation；不一致 ⇒ 显式错误（不是读到别人的数据）。
    fn check(&self, owner: Self::Owner, generation: u32) -> Result<(), HandleError> {
        if self.owner() != owner {
            return Err(HandleError::CrossOwner {
                expected: owner.to_string(),
                actual: self.owner().to_string(),
            });
        }
        if self.generation() != generation {
            return Err(HandleError::Stale {
                slot: self.slot(),
                expected: generation,
                actual: self.generation(),
            });
        }
        Ok(())
    }
}

/// 句柄解析失败（全部显式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandleError {
    /// 句柄属于另一个 owner。
    CrossOwner { expected: String, actual: String },
    /// generation 不匹配：槽位已被复用。
    Stale {
        slot: u64,
        expected: u32,
        actual: u32,
    },
    /// 槽位已释放 / 从未有效。
    Released { slot: u64 },
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CrossOwner { expected, actual } => write!(
                f,
                "跨 owner 句柄：句柄属于 `{actual}`，当前是 `{expected}`"
            ),
            Self::Stale {
                slot,
                expected,
                actual,
            } => write!(
                f,
                "句柄已失效（槽位 {slot}）：期望 generation {expected}，实际 {actual}"
            ),
            Self::Released { slot } => write!(f, "句柄已释放（槽位 {slot}）"),
        }
    }
}

impl std::error::Error for HandleError {}

/// **慢 → 快**的解析协议（计划 §1）。
///
/// 慢形式可序列化、可跨边界、需校验；快形式是进程内、无需再校验的槽位。
/// 失败即显式错误，不静默降级。
pub trait Resolvable {
    /// 可序列化形式。
    type Slow;
    /// 进程内形式。
    type Fast;

    fn resolve(&self, slow: &Self::Slow) -> Result<Self::Fast, ReflectError>;
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// generation 感知的槽位表 —— 全仓**唯一**的句柄 arena 实现。
///
/// `ze-pod-core::HandleTable` 是它的薄包装（owner 绑定 + 类型化句柄），
/// 不再各写一遍 `slots / free / generation`。
pub struct SlotTable<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u64>,
}

impl<T> Default for SlotTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SlotTable<T> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    /// 存入一个值，返回 `(slot, generation)`。
    pub fn insert(&mut self, value: T) -> (u64, u32) {
        if let Some(slot) = self.free.pop() {
            let entry = &mut self.slots[slot as usize];
            entry.generation = entry.generation.wrapping_add(1);
            entry.value = Some(value);
            (slot, entry.generation)
        } else {
            let slot = self.slots.len() as u64;
            self.slots.push(Slot {
                generation: 0,
                value: Some(value),
            });
            (slot, 0)
        }
    }

    /// 取只读引用；generation 不匹配 ⇒ [`HandleError::Stale`]。
    pub fn get(&self, slot: u64, generation: u32) -> Result<&T, HandleError> {
        let entry = self
            .slots
            .get(slot as usize)
            .filter(|entry| entry.generation == generation)
            .ok_or(HandleError::Released { slot })?;
        entry
            .value
            .as_ref()
            .ok_or(HandleError::Released { slot })
    }

    /// 取可变引用；generation 不匹配 ⇒ [`HandleError::Stale`]。
    pub fn get_mut(&mut self, slot: u64, generation: u32) -> Result<&mut T, HandleError> {
        let entry = self
            .slots
            .get_mut(slot as usize)
            .filter(|entry| entry.generation == generation)
            .ok_or(HandleError::Released { slot })?;
        entry
            .value
            .as_mut()
            .ok_or(HandleError::Released { slot })
    }

    /// 显式释放；重复释放同样是显式错误。
    pub fn release(&mut self, slot: u64, generation: u32) -> Result<T, HandleError> {
        let entry = self
            .slots
            .get_mut(slot as usize)
            .filter(|entry| entry.generation == generation)
            .ok_or(HandleError::Released { slot })?;
        let value = entry.value.take().ok_or(HandleError::Released { slot })?;
        self.free.push(slot);
        Ok(value)
    }

    pub fn is_live(&self, slot: u64, generation: u32) -> bool {
        self.get(slot, generation).is_ok()
    }

    /// 存活值数量。
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|slot| slot.value.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 槽位当前 generation（诊断 / 测试用）。
    pub fn generation_of(&self, slot: u64) -> Option<u32> {
        self.slots.get(slot as usize).map(|entry| entry.generation)
    }
}

/// 用一个名字 + 污染源构造重名诊断（各领域注册表的统一入口）。
pub fn duplicate_name(kind: NameKind, name: &str, origin: Blame) -> ReflectError {
    ReflectError::duplicate(kind, name, origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_slot_rejects_the_old_generation() {
        let mut table = SlotTable::new();
        let (slot, generation) = table.insert("first".to_string());
        assert_eq!(table.get(slot, generation).unwrap(), "first");

        let old = table.release(slot, generation).unwrap();
        assert_eq!(old, "first");

        // 复用槽位 ⇒ generation 递增；旧句柄读不到新值。
        let (reused, new_generation) = table.insert("second".to_string());
        assert_eq!((reused, new_generation), (slot, generation + 1));
        assert!(matches!(
            table.get(slot, generation),
            Err(HandleError::Released { .. })
        ));
        assert_eq!(table.get(slot, new_generation).unwrap(), "second");
    }

    #[test]
    fn duplicate_name_is_one_diagnostic_with_a_blame() {
        let error = duplicate_name(NameKind::Type, "acme.Point", Blame::pod("lua"));
        assert_eq!(error.name(), Some("acme.Point"));
        assert_eq!(error.origin(), Blame::pod("lua"));
    }
}
