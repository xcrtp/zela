//! 脏传播：沿出边 BFS，**只返回下游**（源节点不算脏），输出升序。

use std::collections::BTreeSet;

use crate::topology::Topology;

/// 从 `seeds` 沿 successors 传播，返回被污染的下游节点（不含 seed 自身，
/// 除非它确实经非空路径被另一个 seed 再次命中）。
///
/// 这是 `BfsDirtyTracker` 的节点级内核：同一张图 / 同一组 seed ⇒ 同一份升序结果。
pub fn dirty_downstream<T: Topology>(graph: &T, seeds: &[T::Id]) -> Vec<T::Id> {
    let mut visited: BTreeSet<T::Id> = BTreeSet::new();
    let mut queue: Vec<T::Id> = Vec::new();

    for &seed in seeds {
        for target in graph.successors(seed) {
            if visited.insert(target) {
                queue.push(target);
            }
        }
    }

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
    fn propagates_to_all_downstream_but_not_the_source() {
        let graph = Adjacency::from_edges([0u32, 1, 2, 3], [(0, 1), (1, 2), (3, 1)]);
        assert_eq!(dirty_downstream(&graph, &[0]), vec![1, 2]);
        assert_eq!(dirty_downstream(&graph, &[0, 3]), vec![1, 2]);
    }

    #[test]
    fn result_is_deterministic_and_sorted() {
        let graph = Adjacency::from_edges([0u32, 1, 2, 3], [(0, 3), (0, 2), (2, 1)]);
        let first = dirty_downstream(&graph, &[0]);
        assert_eq!(first, vec![1, 2, 3]);
        assert_eq!(first, dirty_downstream(&graph, &[0]));
    }
}
