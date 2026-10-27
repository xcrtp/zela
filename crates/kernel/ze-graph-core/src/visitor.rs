//! **遍历器**：把「多 pass 架构」缺的那一半补上。
//!
//! `Topology` 只描述图；本模块给出一致的**前序 / 后序**遍历（确定性、无 HashMap）。
//! pass 不再各自 `match` 递归，而是拿到一个有序节点序列。

use std::collections::BTreeSet;

use crate::topology::Topology;

/// 访问者：`enter` 返回 `false` 表示不展开该节点的子树。
pub trait Visitor<Id> {
    fn enter(&mut self, id: Id) -> bool;
    fn leave(&mut self, id: Id) {
        let _ = id;
    }
}

/// 前序遍历（DFS，确定性）。
///
/// - 根与后继都按 `Id` 升序（`Topology` 的输出可能无序，这里统一排序）；
/// - 共享节点只访问一次（`visited` 是 `BTreeSet`）；
/// - 返回 `enter` 顺序。
pub fn traverse<T, V>(graph: &T, roots: &[T::Id], visitor: &mut V) -> Vec<T::Id>
where
    T: Topology,
    V: Visitor<T::Id>,
{
    let mut ordered_roots = roots.to_vec();
    ordered_roots.sort_unstable();
    ordered_roots.dedup();

    let mut visited: BTreeSet<T::Id> = BTreeSet::new();
    let mut order = Vec::new();

    for root in ordered_roots {
        if visited.contains(&root) {
            continue;
        }
        // (节点, 是否已展开)
        let mut stack: Vec<(T::Id, bool)> = vec![(root, false)];
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                visitor.leave(id);
                continue;
            }
            if !visited.insert(id) {
                continue;
            }
            if !visitor.enter(id) {
                continue;
            }
            order.push(id);
            stack.push((id, true));
            let mut children = graph.successors(id);
            children.sort_unstable();
            children.dedup();
            for child in children.into_iter().rev() {
                if !visited.contains(&child) {
                    stack.push((child, false));
                }
            }
        }
    }
    order
}

/// 前序序列（不关心 enter/leave）。
pub fn preorder<T: Topology>(graph: &T, roots: &[T::Id]) -> Vec<T::Id> {
    struct Collect<Id>(Vec<Id>);
    impl<Id> Visitor<Id> for Collect<Id> {
        fn enter(&mut self, id: Id) -> bool {
            self.0.push(id);
            true
        }
    }
    let mut collector = Collect(Vec::new());
    traverse(graph, roots, &mut collector);
    collector.0
}

/// 后序序列（子节点先于父节点）。
pub fn postorder<T: Topology>(graph: &T, roots: &[T::Id]) -> Vec<T::Id> {
    struct Collect<Id>(Vec<Id>);
    impl<Id> Visitor<Id> for Collect<Id> {
        fn enter(&mut self, _id: Id) -> bool {
            true
        }
        fn leave(&mut self, id: Id) {
            self.0.push(id);
        }
    }
    let mut collector = Collect(Vec::new());
    traverse(graph, roots, &mut collector);
    collector.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Adjacency;

    fn graph() -> Adjacency<u32> {
        // 0 → {1, 2}；1 → 3；2 → 3（共享汇点）
        Adjacency::from_edges([0u32, 1, 2, 3], [(0, 1), (0, 2), (1, 3), (2, 3)])
    }

    #[test]
    fn preorder_is_deterministic_and_visits_shared_nodes_once() {
        let first = preorder(&graph(), &[0]);
        assert_eq!(first, vec![0, 1, 3, 2]);
        assert_eq!(first, preorder(&graph(), &[0]));
    }

    #[test]
    fn postorder_puts_children_before_parents() {
        let order = postorder(&graph(), &[0]);
        let position = |id: u32| order.iter().position(|&x| x == id).unwrap();
        assert!(position(3) < position(1));
        assert!(position(3) < position(2));
        assert!(position(1) < position(0));
        assert!(position(2) < position(0));
        assert_eq!(order.len(), 4, "共享节点只出现一次");
    }

    #[test]
    fn enter_can_prune_a_subtree() {
        struct SkipTwo;
        impl Visitor<u32> for SkipTwo {
            fn enter(&mut self, id: u32) -> bool {
                id != 2
            }
        }
        let mut visitor = SkipTwo;
        let order = traverse(&graph(), &[0], &mut visitor);
        assert_eq!(order, vec![0, 1, 3]);
    }
}
