//! codata：**生产性展开**（不是图灵完备）。
//!
//! # 无限 ≠ 图灵完备
//!
//! 无限细分曲面是 codata：每次展开**至少产出一个构造器**（productive），
//! 观察者按 `depth` 取有限前沿。它不需要 `fix`，也不需要无界计算。
//!
//! # 三条硬约束
//!
//! 1. **上限进产物**：[`CodataSpec`] 的 `depth` / `max_nodes` 可序列化，
//!    随 payload 走。否则"低端设备降规格"会得到不同产物（W1-01 重演）。
//! 2. **成本有界**：节点预算在展开过程中检查，超出即显式错误（不静默截断）。
//! 3. **缓存必须派生**：本模块不持有任何缓存；每次展开都从头算，
//!    "清空缓存重算逐位相同"因此是结构事实，而不是纪律。
//!
//! # 不生产即错误
//!
//! `step` 返回空列表 ⇒ [`CodataError::NotProductive`]。有限结构请用普通列表，
//! 不要伪装成 codata —— 否则"无限"这个前提就塌了。

use serde::{Deserialize, Serialize};

/// codata 展开参数（**进产物**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodataSpec {
    /// 观察深度（0 = 只看种子）。
    pub depth: u32,
    /// 所有层合计的节点上限（成本上界）。
    pub max_nodes: u32,
}

impl CodataSpec {
    pub const fn new(depth: u32, max_nodes: u32) -> Self {
        Self { depth, max_nodes }
    }
}

/// 展开器：把状态展开成子状态（**必须**至少一个）。
pub trait Unfold {
    type State: Clone;

    /// 展开一层。返回空列表 ⇒ 违反 productivity，展开器会显式失败。
    fn step(&self, state: &Self::State) -> Vec<Self::State>;
}

/// 展开错误：全部显式。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CodataError {
    /// 某层没有产出任何构造器（不是 codata）。
    #[error("展开不生产：第 {level} 层没有产出任何构造器")]
    NotProductive { level: u32 },
    /// 节点总数超过产物上限。
    #[error("节点数超过产物上限 {max_nodes}（depth = {depth}）")]
    BudgetExceeded { max_nodes: u32, depth: u32 },
}

/// 展开结果：`levels[0]` 是种子，`levels[i]` 是第 `i` 层前沿。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expanded<S> {
    pub levels: Vec<Vec<S>>,
}

impl<S> Expanded<S> {
    /// `depth` 观察者：返回最深一层前沿（有限、可直接消费）。
    pub fn frontier(&self) -> &[S] {
        self.levels.last().map(Vec::as_slice).unwrap_or(&[])
    }

    /// 全部构造器数量（成本实测值）。
    pub fn node_count(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

/// 按 [`CodataSpec`] 展开（BFS；每步检查 productivity 与节点预算）。
pub fn expand<U: Unfold>(
    unfold: &U,
    seed: U::State,
    spec: CodataSpec,
) -> Result<Expanded<U::State>, CodataError> {
    let mut levels: Vec<Vec<U::State>> = vec![vec![seed]];
    let mut total: u64 = 1;
    if total > u64::from(spec.max_nodes) {
        return Err(CodataError::BudgetExceeded {
            max_nodes: spec.max_nodes,
            depth: spec.depth,
        });
    }

    for level in 0..spec.depth {
        let mut next = Vec::new();
        for state in levels.last().expect("至少有一层") {
            let children = unfold.step(state);
            if children.is_empty() {
                return Err(CodataError::NotProductive { level: level + 1 });
            }
            for child in children {
                total += 1;
                if total > u64::from(spec.max_nodes) {
                    return Err(CodataError::BudgetExceeded {
                        max_nodes: spec.max_nodes,
                        depth: spec.depth,
                    });
                }
                next.push(child);
            }
        }
        levels.push(next);
    }

    Ok(Expanded { levels })
}

/// 只取 `depth` 处的前沿（不保留中间层的便捷入口；成本口径与 [`expand`] 相同）。
pub fn observe<U: Unfold>(
    unfold: &U,
    seed: U::State,
    spec: CodataSpec,
) -> Result<Vec<U::State>, CodataError> {
    Ok(expand(unfold, seed, spec)?.levels.swap_remove(spec.depth as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无限二叉树：每个节点有两个子节点（值编码路径）。
    struct BinaryTree;

    impl Unfold for BinaryTree {
        type State = u64;

        fn step(&self, state: &u64) -> Vec<u64> {
            // 子节点 = 2s+1 / 2s+2（任意深度都非空 ⇒ productive）。
            vec![state * 2 + 1, state * 2 + 2]
        }
    }

    /// 非生产展开器（有限结构伪装成 codata）。
    struct Stops;

    impl Unfold for Stops {
        type State = u64;

        fn step(&self, state: &u64) -> Vec<u64> {
            if *state == 0 { vec![1] } else { Vec::new() }
        }
    }

    #[test]
    fn depth_observer_returns_the_exact_frontier() {
        let expanded = expand(&BinaryTree, 0, CodataSpec::new(3, 100)).unwrap();
        assert_eq!(expanded.levels.len(), 4);
        assert_eq!(expanded.levels[0], vec![0]);
        assert_eq!(expanded.levels[1], vec![1, 2]);
        assert_eq!(expanded.frontier().len(), 8, "第 3 层有 8 个节点");
        assert_eq!(expanded.node_count(), 1 + 2 + 4 + 8);

        let frontier = observe(&BinaryTree, 0, CodataSpec::new(2, 100)).unwrap();
        assert_eq!(frontier, vec![3, 4, 5, 6]);
    }

    #[test]
    fn expansion_is_deterministic_and_cache_free() {
        let a = expand(&BinaryTree, 0, CodataSpec::new(4, 100)).unwrap();
        let b = expand(&BinaryTree, 0, CodataSpec::new(4, 100)).unwrap();
        assert_eq!(a, b, "同一 spec + 同一 seed ⇒ 逐位相同（无隐藏缓存）");
    }

    #[test]
    fn not_productive_is_an_explicit_error() {
        let error = expand(&Stops, 0, CodataSpec::new(5, 100)).unwrap_err();
        assert_eq!(error, CodataError::NotProductive { level: 2 });
    }

    #[test]
    fn node_budget_is_enforced_during_expansion() {
        // 深度 4 需要 31 个节点；预算 10 ⇒ 显式失败而不是截断。
        let error = expand(&BinaryTree, 0, CodataSpec::new(4, 10)).unwrap_err();
        assert_eq!(
            error,
            CodataError::BudgetExceeded {
                max_nodes: 10,
                depth: 4
            }
        );
    }

    #[test]
    fn spec_is_part_of_the_product() {
        // depth / max_nodes 必须随 payload 序列化（否则降规格会得到不同产物）。
        let spec = CodataSpec::new(6, 4096);
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(json, r#"{"depth":6,"max_nodes":4096}"#);
        let restored: CodataSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, spec);
    }

    /// 成本有界：给定 spec 时，要么节点数不超过 `max_nodes`，要么**显式失败**。
    /// 不存在"超出预算但静默截断"的第三条路径。
    #[test]
    fn cost_is_bounded_by_the_spec() {
        let spec = CodataSpec::new(10, 512);
        match expand(&BinaryTree, 0, spec) {
            Ok(expanded) => assert!(expanded.node_count() <= 512),
            Err(error) => assert_eq!(
                error,
                CodataError::BudgetExceeded {
                    max_nodes: 512,
                    depth: 10
                }
            ),
        }
    }
}
