//! **表达式图分析**：`ze-graph-core` 的 SCC / reachable / dirty 在表达式 DAG 上的落点。
//!
//! # 为什么这里不是"再来一套算法"
//!
//! 全部是 `ze-graph-core` 的组合：
//! - 共享定义 ⇒ 图视图的**入度**（`predecessors`）；
//! - 受影响消费者 ⇒ **反向图**上的 `dirty_downstream`（正向边表示"依赖"，反向才是"受影响"）；
//! - 死代码 ⇒ `reachable` 的补集；
//! - 互引分组 ⇒ `strongly_connected`。
//!
//! # 确定性
//!
//! 输出按 id 升序（无 id 的中间节点不进入结果），与图内存储顺序无关。
//! 不引入 `HashMap`（硬约束 2）。

use std::collections::{BTreeMap, BTreeSet};

use ze_graph_core::{Topology, dirty_downstream, reachable, strongly_connected};

use crate::expr::{Expr, Id};
use crate::graph_view::ExprGraphView;

/// 共享定义：被 **≥ 2 个不同消费者**引用的 `Def`，`(id, 消费者数)` 按 id 升序。
///
/// 这是"每帧只求值一次"的收益清单：消费者越多，共享越值钱。
pub fn shared_definitions(expr: &Expr) -> Vec<(&str, usize)> {
    let view = ExprGraphView::build(expr);
    let mut shared: BTreeMap<&str, usize> = BTreeMap::new();
    for (id, &index) in view.definitions() {
        let consumers = view.predecessors(index).len();
        if consumers > 1 {
            shared.insert(id, consumers);
        }
    }
    shared.into_iter().collect()
}

/// 受影响消费者：`changed` 这些 `Def` 变化后，沿**反向边**传播到的节点 id（升序）。
///
/// 反向传播正是增量重算需要的方向：`changed` 是"源变了"，结果是"谁要重算"。
pub fn affected_consumers(expr: &Expr, changed: &[&str]) -> Vec<Id> {
    let view = ExprGraphView::build(expr);
    let seeds: Vec<usize> = changed
        .iter()
        .filter_map(|id| view.definitions().get(id).copied())
        .collect();
    if seeds.is_empty() {
        return Vec::new();
    }
    let mut affected: Vec<Id> = dirty_downstream(&Reversed(&view), &seeds)
        .into_iter()
        .filter_map(|index| view.expr(index).and_then(|node| node.id.clone()))
        .collect();
    affected.sort();
    affected
}

/// 从 `roots` **不可达**的节点下标（死代码 / 未连接定义），升序。
///
/// 根为 `[view.root()]` 时结果恒为空（词法树全部来自根）；
/// 局部观察点（例如某个绑定关心的 `Def`）才是它的用武之地。
pub fn unreachable(expr: &Expr, roots: &[usize]) -> Vec<usize> {
    let view = ExprGraphView::build(expr);
    let mut alive: BTreeSet<usize> = BTreeSet::new();
    for &root in roots {
        alive.extend(reachable(&view, root));
    }
    (0..view.len())
        .filter(|index| !alive.contains(index))
        .collect()
}

/// 互引分组（SCC）：只返回**成环**的分量（自环算一个），组内与组间均按 id 升序。
///
/// `Expr::validate` 已拒绝 `Ref` 成环，因此正常产物恒为空；
/// 它服务于**诊断**（与 [`crate::ExprError::Cycle`] 同一份图）。
pub fn reference_groups(expr: &Expr) -> Vec<Vec<&str>> {
    let view = ExprGraphView::build(expr);
    let mut groups: Vec<Vec<&str>> = Vec::new();
    for component in strongly_connected(&view) {
        let cyclic = component.len() > 1 || {
            let node = component[0];
            view.successors(node).contains(&node)
        };
        if !cyclic {
            continue;
        }
        let names: Vec<&str> = component
            .iter()
            .filter_map(|&index| view.expr(index).and_then(|node| node.id.as_deref()))
            .collect();
        if !names.is_empty() {
            groups.push(names);
        }
    }
    groups
}

/// 反向图：`successors` / `predecessors` 互换（只用于脏传播的方向修正）。
struct Reversed<'a, 'v>(&'v ExprGraphView<'a>);

impl Topology for Reversed<'_, '_> {
    type Id = usize;

    fn nodes(&self) -> Vec<usize> {
        self.0.nodes()
    }

    fn successors(&self, node: usize) -> Vec<usize> {
        self.0.predecessors(node)
    }

    fn predecessors(&self, node: usize) -> Vec<usize> {
        self.0.successors(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::OpRef;

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    /// 不依赖渲染的 fixture：
    /// `root = add(b, c)`、`b = add(a, 1)`、`c = add(ref a, 2)`、`a = pure(1)`。
    fn fixture() -> Expr {
        let a = Expr::pure(1.0).with_id("a");
        let b = Expr::call(math("add"), [a, Expr::pure(1.0)]).with_id("b");
        let c = Expr::call(math("add"), [Expr::reference("a"), Expr::pure(2.0)]).with_id("c");
        Expr::call(math("add"), [b, c])
    }

    #[test]
    fn shared_definitions_counts_distinct_consumers() {
        assert_eq!(shared_definitions(&fixture()), vec![("a", 2)]);
    }

    #[test]
    fn affected_consumers_walks_the_reverse_edges() {
        // `a` 变化 ⇒ 直接消费者 `b` / `c`（根无 id，不进入结果）。
        assert_eq!(affected_consumers(&fixture(), &["a"]), vec!["b", "c"]);
        // 未知 id 不是错误，但也没有受影响者。
        assert!(affected_consumers(&fixture(), &["missing"]).is_empty());
    }

    #[test]
    fn unreachable_reports_the_complement_of_reachability() {
        let expr = fixture();
        let view = ExprGraphView::build(&expr);
        let b = view.definitions()["b"];
        let dead: Vec<Option<&str>> = unreachable(&expr, &[b])
            .into_iter()
            .map(|index| view.expr(index).and_then(|node| node.id.as_deref()))
            .collect();
        // 从 `b` 只到 `a` 与 `b` 自己的实参 ⇒ 根与 `c` 的实参不可达。
        assert_eq!(dead, vec![None, Some("c"), None]);
    }

    #[test]
    fn reference_groups_is_empty_for_a_dag_and_names_a_self_loop() {
        assert!(reference_groups(&fixture()).is_empty());

        let cyclic = Expr::call(math("add"), [Expr::reference("x")]).with_id("x");
        assert_eq!(reference_groups(&cyclic), vec![vec!["x"]]);
    }
}
