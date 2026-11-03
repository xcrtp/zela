//! 可迭代类型：**`Iter`（有穷）与 `Stream`（余归纳）是两个类型**。
//!
//! # 为什么不能合并
//!
//! ```text
//! NatRec   归纳 · 有穷 · count 在递归外 ⇒ 折叠成一个值   ⇒ Iter（长度已知）
//! codata   余归纳 · 潜在无限 ⇒ 逐步展开                  ⇒ Stream
//! ```
//!
//! 范畴论上二者是对偶的（catamorphism / anamorphism），**载体可以共享，
//! 终止性不可以合并**：合并成"看起来普通的迭代器"就会丢掉 `NatRec` 的终止保证 ——
//! 而那正是本项拒绝图灵完备、接受原始递归的核心理由。
//!
//! # 上界进产物（同一模式的第五次）
//!
//! Σ 上界 · codata 的 `depth` · 求解器 `dt` · `MAX_INSTANCES` · 现在是 `Stream` 的
//! `depth`：观察深度**随 [`ValueStream`] 一起走**，越界**显式失败**
//! （[`EvalError::StreamDepth`]），不静默截断。

use serde::{Deserialize, Serialize};

use crate::codata::Expanded;
use crate::program::{EvalError, Value};

/// 有穷序列：**长度已知** ⇒ 可以折叠成一个值。
///
/// 元素必须**同型**（`kind()` 相同）：`Iter<T>` 的 `T` 不是装饰，`sum` / `map`
/// 都依赖它。混型在构造处显式失败。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueIter {
    items: Vec<Value>,
}

impl ValueIter {
    /// 由已知长度的序列构造（混型 ⇒ [`EvalError::IterElemType`]）。
    pub fn new(items: Vec<Value>) -> Result<Self, EvalError> {
        let mut expected: Option<&'static str> = None;
        for (index, item) in items.iter().enumerate() {
            let actual = item.kind();
            match expected {
                None => expected = Some(actual),
                Some(expected) if expected == actual => {}
                Some(expected) => {
                    return Err(EvalError::IterElemType {
                        index,
                        expected,
                        actual,
                    })
                }
            }
        }
        Ok(Self { items })
    }

    /// 长度（已知 ⇒ 折叠不需要预算）。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 元素（只读）。
    pub fn items(&self) -> &[Value] {
        &self.items
    }

    /// 元素种类；空序列没有种类（`None`）。
    pub fn elem_kind(&self) -> Option<&'static str> {
        self.items.first().map(Value::kind)
    }

    /// **折叠**（catamorphism）：`init` 是第 0 个状态，逐元素左折。
    ///
    /// 加法顺序与 `NatRec` 的 `step(prev, i)` **完全一致**（`((init ⊕ x1) ⊕ x2) …`）
    /// ⇒ 与手写展开 / `NatRec` 求值**逐位相同**（不是容差内）。
    pub fn fold<F>(&self, init: Value, mut step: F) -> Result<Value, EvalError>
    where
        F: FnMut(Value, &Value, usize) -> Result<Value, EvalError>,
    {
        let mut state = init;
        for (index, item) in self.items.iter().enumerate() {
            state = step(state, item, index)?;
        }
        Ok(state)
    }

    /// 逐元素映射（结果仍是 `Iter`：长度不变 ⇒ 仍然是归纳侧）。
    pub fn map<F>(&self, mut f: F) -> Result<Self, EvalError>
    where
        F: FnMut(&Value, usize) -> Result<Value, EvalError>,
    {
        let mut items = Vec::with_capacity(self.items.len());
        for (index, item) in self.items.iter().enumerate() {
            items.push(f(item, index)?);
        }
        Self::new(items)
    }

    /// 求和：与 `zela.math::sum` 同一口径（`f64` 累加、空序列 = `0.0`）。
    ///
    /// 非数值元素 ⇒ [`EvalError::IterNotSummable`]（不做隐式提升）。
    pub fn sum(&self) -> Result<Value, EvalError> {
        let total = self.fold(Value::f64(0.0), |acc, item, _| {
            let acc = acc.as_f64().expect("sum 的状态恒为 f64");
            let item = item
                .as_f64()
                .ok_or(EvalError::IterNotSummable { actual: item.kind() })?;
            Ok(Value::f64(acc + item))
        })?;
        Ok(total)
    }
}

/// 余归纳前沿：**潜在无限**，但每次观察有界。
///
/// `depth` 与 `levels` 都随值走（`levels.len()` 恒为 `depth + 1`）；
/// 越过 `depth` 的观察是 [`EvalError::StreamDepth`]，不是"返回空"。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueStream {
    depth: u32,
    levels: Vec<Vec<Value>>,
}

impl ValueStream {
    /// 由**已展开**的层构造（层数必须恰好 `depth + 1`）。
    pub fn new(depth: u32, levels: Vec<Vec<Value>>) -> Result<Self, EvalError> {
        if levels.len() != depth as usize + 1 {
            return Err(EvalError::StreamShape {
                depth,
                levels: levels.len(),
            });
        }
        Ok(Self { depth, levels })
    }

    /// codata 展开结果 → `Stream`（上界取自 `levels` 的实际层数）。
    ///
    /// 与 [`crate::codata::expand`] 同一上界：`depth` 就是产物里那份 `CodataSpec.depth`。
    pub fn from_expanded(expanded: Expanded<Value>) -> Result<Self, EvalError> {
        let depth = (expanded.levels.len() as u32).saturating_sub(1);
        Self::new(depth, expanded.levels)
    }

    /// 产物里登记的上界。
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// 第 `level` 层前沿（`level == 0` 是种子）。
    ///
    /// `level > depth` ⇒ [`EvalError::StreamDepth`]：**显式失败，不截断**。
    pub fn frontier(&self, level: u32) -> Result<&[Value], EvalError> {
        if level > self.depth {
            return Err(EvalError::StreamDepth {
                requested: level,
                depth: self.depth,
            });
        }
        Ok(&self.levels[level as usize])
    }

    /// 全部层（只读；诊断 / 结构指纹用）。
    pub fn levels(&self) -> &[Vec<Value>] {
        &self.levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codata::{CodataSpec, Unfold, expand};

    fn numbers(values: &[f64]) -> Vec<Value> {
        values.iter().copied().map(Value::f64).collect()
    }

    #[test]
    fn iter_fold_is_left_fold_in_order() {
        let iter = ValueIter::new(numbers(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(iter.len(), 3);
        assert_eq!(iter.elem_kind(), Some("scalar"));
        assert_eq!(iter.sum().unwrap(), Value::f64(6.0));

        // 折叠顺序：(0 + 1) + 2 + 3，与 NatRec / 手写展开同一结合顺序。
        let folded = iter
            .fold(Value::f64(10.0), |acc, item, _| {
                Ok(Value::f64(acc.as_f64().unwrap() + item.as_f64().unwrap()))
            })
            .unwrap();
        assert_eq!(folded, Value::f64(16.0));
    }

    #[test]
    fn iter_is_length_known_and_empty_sum_is_zero() {
        let empty = ValueIter::new(Vec::new()).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.elem_kind(), None);
        assert_eq!(empty.sum().unwrap(), Value::f64(0.0));
    }

    #[test]
    fn iter_rejects_mixed_element_kinds() {
        let error = ValueIter::new(vec![Value::f64(1.0), Value::Bool(true)]).unwrap_err();
        assert_eq!(
            error,
            EvalError::IterElemType {
                index: 1,
                expected: "scalar",
                actual: "bool",
            }
        );
    }

    #[test]
    fn iter_map_keeps_length_and_type() {
        let iter = ValueIter::new(numbers(&[1.0, 2.0])).unwrap();
        let doubled = iter
            .map(|item, _| Ok(Value::f64(item.as_f64().unwrap() * 2.0)))
            .unwrap();
        assert_eq!(doubled.len(), 2);
        assert_eq!(doubled.items(), numbers(&[2.0, 4.0]));
    }

    #[test]
    fn sum_rejects_non_numeric_elements() {
        let iter = ValueIter::new(vec![Value::Bool(true)]).unwrap();
        assert_eq!(
            iter.sum().unwrap_err(),
            EvalError::IterNotSummable { actual: "bool" }
        );
    }

    /// 无限二叉树：深度观察有界。
    struct BinaryTree;

    impl Unfold for BinaryTree {
        type State = Value;

        fn step(&self, state: &Value) -> Vec<Value> {
            let value = state.as_f64().expect("种子是数值");
            vec![Value::f64(value * 2.0 + 1.0), Value::f64(value * 2.0 + 2.0)]
        }
    }

    #[test]
    fn stream_keeps_its_depth_bound_in_the_product() {
        let expanded = expand(&BinaryTree, Value::f64(0.0), CodataSpec::new(2, 100)).unwrap();
        let stream = ValueStream::from_expanded(expanded).unwrap();
        assert_eq!(stream.depth(), 2);
        assert_eq!(stream.levels().len(), 3);
        assert_eq!(stream.frontier(0).unwrap(), numbers(&[0.0]));
        assert_eq!(stream.frontier(2).unwrap().len(), 4);

        // 上界是产物的一部分：序列化后仍在。
        let json = serde_json::to_string(&stream).unwrap();
        let restored: ValueStream = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, stream);
        assert_eq!(restored.depth(), 2);
    }

    #[test]
    fn observing_beyond_the_bound_fails_explicitly() {
        let stream = ValueStream::new(1, vec![numbers(&[0.0]), numbers(&[1.0, 2.0])]).unwrap();
        assert_eq!(
            stream.frontier(2).unwrap_err(),
            EvalError::StreamDepth {
                requested: 2,
                depth: 1
            },
            "超界必须显式失败（不是返回空 / 静默截断）"
        );
    }

    #[test]
    fn stream_shape_must_match_the_bound() {
        assert_eq!(
            ValueStream::new(3, vec![numbers(&[0.0])]).unwrap_err(),
            EvalError::StreamShape {
                depth: 3,
                levels: 1
            }
        );
    }
}
