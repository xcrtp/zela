//! 强连通分量（Kosaraju，**迭代实现** + 确定性输出）。
//!
//! 为什么不是递归 Tarjan：图的规模没有上界，递归深度会随链长增长。
//! Kosaraju 的两遍 DFS 都可以显式栈实现，且输出天然可以排序。

use std::collections::{BTreeMap, BTreeSet};

use crate::topology::Topology;

/// 全部 SCC：每个分量内按 id 升序，分量之间按最小 id 升序。
pub fn strongly_connected<T: Topology>(graph: &T) -> Vec<Vec<T::Id>> {
    let nodes: BTreeSet<T::Id> = graph.nodes().into_iter().collect();
    if nodes.is_empty() {
        return Vec::new();
    }

    let successors = adjacency(graph, &nodes, false);
    let predecessors = adjacency(graph, &nodes, true);

    let finish = finish_order(&nodes, &successors);
    let mut visited: BTreeSet<T::Id> = BTreeSet::new();
    let mut components: Vec<Vec<T::Id>> = Vec::new();

    for &start in finish.iter().rev() {
        if !visited.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            component.push(node);
            for &next in predecessors.get(&node).expect("邻接表完整") {
                if visited.insert(next) {
                    stack.push(next);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }

    components.sort_unstable_by(|left, right| left[0].cmp(&right[0]));
    components
}

/// 找到**一个**环，返回**按边顺序**的路径（`v0 → v1 → … → v0`，不重复首节点）。
///
/// 多个环时取"最小 id 最小的那个分量"；分量内从最小 id 出发走一条回到自身的路径。
/// 输出确定：与图内存储顺序无关。
pub fn find_cycle<T: Topology>(graph: &T) -> Option<Vec<T::Id>> {
    let components = strongly_connected(graph);
    let mut candidates: Vec<Vec<T::Id>> = Vec::new();
    for component in components {
        if component.len() > 1 {
            candidates.push(component);
            continue;
        }
        let node = component[0];
        if graph.successors(node).contains(&node) {
            candidates.push(component);
        }
    }
    candidates.sort_unstable_by(|left, right| left[0].cmp(&right[0]));
    let component = candidates.into_iter().next()?;
    Some(cycle_path(graph, &component))
}

/// 在强连通分量内找一条从 `start` 回到 `start` 的路径（不含重复的终点）。
fn cycle_path<T: Topology>(graph: &T, component: &[T::Id]) -> Vec<T::Id> {
    let start = component[0];
    let members: BTreeSet<T::Id> = component.iter().copied().collect();
    let mut neighbors: BTreeMap<T::Id, Vec<T::Id>> = BTreeMap::new();
    for &node in &members {
        let mut targets: Vec<T::Id> = graph
            .successors(node)
            .into_iter()
            .filter(|target| members.contains(target))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        neighbors.insert(node, targets);
    }

    // 自环。
    if neighbors
        .get(&start)
        .is_some_and(|targets| targets.contains(&start))
    {
        return vec![start];
    }

    // 取 start 的第一条出边，再 BFS 回到 start（强连通 ⇒ 必然存在）。
    for &first in neighbors.get(&start).expect("邻接表完整") {
        let mut parent: BTreeMap<T::Id, T::Id> = BTreeMap::new();
        let mut visited: BTreeSet<T::Id> = BTreeSet::new();
        let mut queue: Vec<T::Id> = vec![first];
        visited.insert(first);
        let mut cursor = 0;
        while cursor < queue.len() {
            let node = queue[cursor];
            cursor += 1;
            if node == start {
                break;
            }
            for &next in neighbors.get(&node).expect("邻接表完整") {
                if visited.insert(next) {
                    parent.insert(next, node);
                    queue.push(next);
                }
            }
        }
        if !visited.contains(&start) {
            continue;
        }
        let mut path = Vec::new();
        let mut node = start;
        loop {
            path.push(node);
            let Some(&previous) = parent.get(&node) else {
                break;
            };
            node = previous;
        }
        path.reverse();
        // 旋转到从最小 id 开始：`[start, first, …, last]`。
        if let Some(last) = path.pop() {
            path.insert(0, last);
        }
        return path;
    }
    // 理论上不可达：分量强连通。
    component.to_vec()
}

/// 邻接表（去重、升序）；`reverse = true` 时用 `predecessors`。
fn adjacency<T: Topology>(
    graph: &T,
    nodes: &BTreeSet<T::Id>,
    reverse: bool,
) -> BTreeMap<T::Id, Vec<T::Id>> {
    let mut map: BTreeMap<T::Id, Vec<T::Id>> = nodes.iter().map(|&node| (node, Vec::new())).collect();
    for &node in nodes {
        let edges = if reverse {
            graph.predecessors(node)
        } else {
            graph.successors(node)
        };
        let entry = map.get_mut(&node).expect("已初始化");
        for target in edges {
            if nodes.contains(&target) {
                entry.push(target);
            }
        }
        entry.sort_unstable();
        entry.dedup();
    }
    map
}

/// 第一遍 DFS 的完成顺序（显式栈）。
fn finish_order<Id: Copy + Ord + std::fmt::Debug>(
    nodes: &BTreeSet<Id>,
    successors: &BTreeMap<Id, Vec<Id>>,
) -> Vec<Id> {
    let mut visited: BTreeSet<Id> = BTreeSet::new();
    let mut finish = Vec::with_capacity(nodes.len());
    let mut next_index: BTreeMap<Id, usize> = BTreeMap::new();

    for &start in nodes {
        if !visited.insert(start) {
            continue;
        }
        let mut stack: Vec<Id> = vec![start];
        while let Some(&node) = stack.last() {
            let neighbors = successors.get(&node).expect("邻接表完整");
            let index = next_index.entry(node).or_insert(0);
            if *index < neighbors.len() {
                let target = neighbors[*index];
                *index += 1;
                if visited.insert(target) {
                    stack.push(target);
                }
            } else {
                stack.pop();
                next_index.remove(&node);
                finish.push(node);
            }
        }
    }
    finish
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Adjacency;

    #[test]
    fn separates_two_cycles_and_a_tail() {
        // 环 1：0↔1；环 2：3→4→5→3；6 单独。
        let graph = Adjacency::from_edges(
            [0u32, 1, 2, 3, 4, 5, 6],
            [(0, 1), (1, 0), (1, 2), (3, 4), (4, 5), (5, 3)],
        );
        let components = strongly_connected(&graph);
        assert_eq!(
            components,
            vec![vec![0, 1], vec![2], vec![3, 4, 5], vec![6]]
        );
    }

    #[test]
    fn cycle_detection_is_deterministic() {
        let graph = Adjacency::from_edges([0u32, 1, 2, 3], [(3, 2), (2, 1), (1, 2)]);
        assert_eq!(find_cycle(&graph), Some(vec![1, 2]));
        assert_eq!(find_cycle(&graph), find_cycle(&graph));
    }

    #[test]
    fn self_loop_is_a_cycle() {
        let graph = Adjacency::from_edges([7u32], [(7, 7)]);
        assert_eq!(find_cycle(&graph), Some(vec![7]));
    }

    #[test]
    fn dag_has_no_cycle() {
        let graph = Adjacency::from_edges([0u32, 1, 2], [(0, 1), (1, 2)]);
        assert_eq!(find_cycle(&graph), None);
    }
}
