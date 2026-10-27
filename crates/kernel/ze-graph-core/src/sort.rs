//! Kahn 拓扑排序（**确定性**：就绪集合取最小 id）。

use std::collections::{BTreeMap, BTreeSet};

use crate::topology::Topology;

/// 成环时的显式错误：剩余节点（升序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CycleNodes<Id> {
    pub nodes: Vec<Id>,
}

/// 拓扑排序：上游在前；同一层内按 id 升序。
///
/// 返回顺序**只由拓扑与 id 决定**，与 `nodes()` / `successors()` 的返回顺序无关。
pub fn topo_sort<T: Topology>(graph: &T) -> Result<Vec<T::Id>, CycleNodes<T::Id>> {
    let nodes: BTreeSet<T::Id> = graph.nodes().into_iter().collect();
    let mut in_degree: BTreeMap<T::Id, usize> =
        nodes.iter().map(|&node| (node, 0usize)).collect();
    let mut successors: BTreeMap<T::Id, BTreeSet<T::Id>> =
        nodes.iter().map(|&node| (node, BTreeSet::new())).collect();

    for &node in &nodes {
        for target in graph.successors(node) {
            if !nodes.contains(&target) {
                continue;
            }
            if successors.get_mut(&node).expect("已初始化").insert(target) {
                *in_degree.get_mut(&target).expect("已初始化") += 1;
            }
        }
    }

    // 就绪集合用 BTreeSet：`pop_first` = 最小 id ⇒ 输出确定。
    let mut ready: BTreeSet<T::Id> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(&node, _)| node)
        .collect();

    let mut order = Vec::with_capacity(nodes.len());
    while let Some(node) = ready.pop_first() {
        order.push(node);
        if let Some(targets) = successors.get(&node) {
            for &target in targets {
                let degree = in_degree.get_mut(&target).expect("已初始化");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(target);
                }
            }
        }
    }

    if order.len() == nodes.len() {
        Ok(order)
    } else {
        let placed: BTreeSet<T::Id> = order.iter().copied().collect();
        Err(CycleNodes {
            nodes: nodes.difference(&placed).copied().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Adjacency;

    fn shuffled_graph() -> Adjacency<u32> {
        // 插入顺序故意与 id 顺序相反：输出仍必须按 id 决定。
        let mut graph = Adjacency::new();
        graph.add_edge(3, 4);
        graph.add_edge(1, 3);
        graph.add_edge(2, 4);
        graph.add_node(0);
        graph
    }

    #[test]
    fn sorts_upstream_first() {
        let order = topo_sort(&shuffled_graph()).expect("DAG");
        let position = |id: u32| order.iter().position(|&x| x == id).unwrap();
        assert!(position(1) < position(3));
        assert!(position(3) < position(4));
        assert!(position(2) < position(4));
    }

    #[test]
    fn output_is_deterministic_across_insertion_orders() {
        let mut other = Adjacency::new();
        other.add_node(0);
        other.add_edge(2, 4);
        other.add_edge(1, 3);
        other.add_edge(3, 4);
        assert_eq!(
            topo_sort(&shuffled_graph()).unwrap(),
            topo_sort(&other).unwrap(),
            "同一张图的拓扑序必须逐位相同"
        );
    }

    #[test]
    fn cycle_is_an_explicit_error_with_sorted_nodes() {
        let graph = Adjacency::from_edges([0u32, 1, 2], [(0, 1), (1, 0), (2, 2)]);
        let error = topo_sort(&graph).unwrap_err();
        assert_eq!(error.nodes, vec![0, 1, 2]);
    }
}
