//! resolve.rs — **字段级**写入仲裁（CC-c）。
//!
//! # 为什么是字段级
//!
//! 冲突的本质是"两个写入者写同一字段"；系统级开关（整条绑定移除 / 整类目标禁用）
//! 会把"用户接管相机"这种局部事件放大成二选一。这里按 `(对象, 字段地址)` 粒度
//! 择优：同对象上写不同字段的绑定互不影响。
//!
//! # 优先级
//!
//! 优先级**声明在绑定侧**（[`Binding::priority`](crate::animation::binding::Binding::priority)），
//! 不集中成表（集中表会与绑定表漂移）。规则：数值大者胜；**同优先级不抑制**
//! （按声明顺序依次写入，后声明者自然覆盖 —— 与既有行为逐位一致）。
//!
//! # 复合目标
//!
//! `Camera2DFraming` / `RevealRange` 一个写入者写两个字段（见
//! [`Target::write_fields`](crate::animation::Target::write_fields)）。仲裁对它们
//! 采用**全或无**：任一字段被更高优先级者赢走，整条写入跳过 —— 避免"写一半"。
//!
//! # 可观测
//!
//! 被抑制的写入逐条记进 [`WriteResolution::suppressed`]（来源 + 字段 + 胜者），
//! 由诊断面板 / 用例读取；不静默丢弃。

use std::collections::BTreeMap;

use ze_scene_contract::ObjectId;

use crate::animation::binding::{Binding, BindingTable};

/// 一次被抑制的写入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressedWrite {
    /// 目标对象。
    pub object: ObjectId,
    /// 字段地址（`Circle.radius` / `Framing2D.scale` …）。
    pub field: &'static str,
    /// 被抑制的来源（绑定目标名）。
    pub source: String,
    /// 胜者（写入同一字段的绑定目标名）。
    pub winner: String,
}

/// 一帧的仲裁结果（Resource；每帧开头清空）。
#[derive(Debug, Default, Clone)]
pub struct WriteResolution {
    /// 本帧被抑制的写入（按绑定顺序）。
    pub suppressed: Vec<SuppressedWrite>,
}

impl WriteResolution {
    /// 帧首清空（由 [`BindingApplySystem`](crate::animation::system::BindingApplySystem) 调用）。
    pub fn begin_frame(&mut self) {
        self.suppressed.clear();
    }
}

/// 字段 → `(优先级, 绑定下标)`（预计算，供逐条判定）。
pub type FieldWinners = BTreeMap<(ObjectId, &'static str), (u8, usize)>;

/// 预计算每个字段的**最高优先级**（同优先级保留后声明者仅用于显示胜者名）。
pub fn field_winners(table: &BindingTable) -> FieldWinners {
    let mut winners = FieldWinners::new();
    for (index, binding) in table.bindings.iter().enumerate() {
        for field in binding.target.write_fields() {
            let key = (binding.target.target_id(), field);
            match winners.get_mut(&key) {
                Some(entry) if binding.priority >= entry.0 => *entry = (binding.priority, index),
                Some(_) => {}
                None => {
                    winners.insert(key, (binding.priority, index));
                }
            }
        }
    }
    winners
}

/// 该绑定是否**没有被更高优先级者压过**（复合目标全或无）。
///
/// 同优先级不算压制：按声明顺序写入即可（保留既有覆盖语义）。
pub fn wins_all(binding: &Binding, _index: usize, winners: &FieldWinners) -> bool {
    let object = binding.target.target_id();
    binding.target.write_fields().into_iter().all(|field| {
        winners
            .get(&(object, field))
            .is_none_or(|(priority, _)| binding.priority >= *priority)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::animation::binding::{BindingDriver, ResetBehavior, SeekKind};
    use crate::animation::target::{FieldTarget, target_entry};
    use ze_time::{ClockId, Curve};

    fn object(index: u32) -> ObjectId {
        ObjectId {
            index,
            generation: 1,
        }
    }

    fn field_binding(id: ObjectId, field: &'static str, priority: u8) -> Binding {
        Binding {
            clock: ClockId(0),
            scope: crate::animation::ScopeId::ROOT,
            priority,
            driver: BindingDriver::Curve(Curve::Const(ze_time::AnimValue::Scalar(0.0))),
            target: Arc::new(FieldTarget {
                id,
                spec: target_entry(field).expect("字段已登记").spec,
            }),
            predicate: None,
            active: None,
            reset: ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        }
    }

    #[test]
    fn higher_priority_wins_the_field() {
        let mut table = BindingTable::new();
        table.add(field_binding(object(1), "Circle.radius", 0));
        table.add(field_binding(object(1), "Circle.radius", 5));
        let winners = field_winners(&table);
        assert!(wins_all(&table.bindings[1], 1, &winners));
        assert!(!wins_all(&table.bindings[0], 0, &winners));
    }

    #[test]
    fn equal_priority_never_suppresses() {
        let mut table = BindingTable::new();
        table.add(field_binding(object(1), "Circle.radius", 0));
        table.add(field_binding(object(1), "Circle.radius", 0));
        let winners = field_winners(&table);
        // 同优先级按声明顺序写入；后者自然覆盖，不产生"被抑制"。
        assert!(wins_all(&table.bindings[0], 0, &winners));
        assert!(wins_all(&table.bindings[1], 1, &winners));
    }

    #[test]
    fn different_fields_do_not_conflict() {
        let mut table = BindingTable::new();
        table.add(field_binding(object(1), "Circle.radius", 0));
        table.add(field_binding(object(1), "Circle.center", 0));
        let winners = field_winners(&table);
        assert!(wins_all(&table.bindings[0], 0, &winners));
        assert!(wins_all(&table.bindings[1], 1, &winners));
    }

    #[test]
    fn different_objects_do_not_conflict() {
        let mut table = BindingTable::new();
        table.add(field_binding(object(1), "Circle.radius", 0));
        table.add(field_binding(object(2), "Circle.radius", 0));
        let winners = field_winners(&table);
        assert!(wins_all(&table.bindings[0], 0, &winners));
        assert!(wins_all(&table.bindings[1], 1, &winners));
    }

    /// 复合目标：写两个字段；只要一个字段被更高优先级赢走，整条跳过（全或无）。
    #[test]
    fn composite_target_is_all_or_nothing() {
        let mut table = BindingTable::new();
        table.add(field_binding(object(1), "Framing2D.scale", 5));
        let composite = Binding {
            clock: ClockId(0),
            scope: crate::animation::ScopeId::ROOT,
            priority: 0,
            driver: BindingDriver::Curve(Curve::Const(ze_time::AnimValue::Scalar(0.0))),
            target: Arc::new(crate::animation::Camera2DFraming { id: object(1) }),
            predicate: None,
            active: None,
            reset: ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        };
        table.add(composite);
        let winners = field_winners(&table);
        assert!(!wins_all(&table.bindings[1], 1, &winners), "scale 被赢走 ⇒ 整条跳过");
        // 单字段仍归它自己。
        assert!(wins_all(&table.bindings[0], 0, &winners));
    }
}
