//! 可达性：单向 BFS（**输出升序**）。

use std::collections::BTreeSet;

use crate::topology::Topology;

/// 从 `from` 出发可达的全部节点（含 `from` 自身），按 id 升序。
pub fn reachable<T: Topology>(graph: &T, from: T::Id) -> Vec<T::Id> {
    let mut visited: BTreeSet<T::Id> = BTreeSet::new();
    visited.insert(from);
    let mut queue: Vec<T::Id> = vec![from];
    let mut cursor = 0;
    while cursor < queue.len() {
        let node = queue[cursor];
        cursor += 1;
        for target in graph.successors(node) {
            if visited.insert(target) {
                queue.push(target);
            }
        }
    }
    visited.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Adjacency;

    #[test]
    fn reachable_is_sorted_and_complete() {
        let graph = Adjacency::from_edges([0u32, 1, 2, 3, 4], [(0, 3), (3, 1), (1, 2)]);
        assert_eq!(reachable(&graph, 0), vec![0, 1, 2, 3]);
        assert_eq!(reachable(&graph, 4), vec![4]);
    }

    #[test]
    fn reachable_is_stable_across_runs() {
        let graph = Adjacency::from_edges([0u32, 1, 2], [(2, 0), (0, 1)]);
        assert_eq!(reachable(&graph, 2), vec![0, 1, 2]);
        assert_eq!(reachable(&graph, 2), reachable(&graph, 2));
    }
}
