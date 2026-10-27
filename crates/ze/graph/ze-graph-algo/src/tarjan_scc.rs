//! Tarjan's strongly-connected-components algorithm, used for cycle detection.
//!
//! **Deterministic**（硬约束 1）：邻接表 / 顶点栈 / 根集合全部用 `BTree*`，
//! 分量内与分量间都按 `NodeId` 排序 —— 同一张图跑两次输出逐位相同。

use std::collections::{BTreeMap, BTreeSet};

use ze_node_core::algo::CycleDetector;
use ze_node_core::graph::Graph;
use ze_node_core::id::{InputSocketId, NodeId};

/// Cycle detector based on Tarjan's SCC algorithm.
#[derive(Debug, Default)]
pub struct TarjanSccDetector;

impl TarjanSccDetector {
    pub fn new() -> Self {
        Self
    }

    fn build_successors(graph: &dyn Graph) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
        let mut succ: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for n in graph.all_nodes() {
            succ.entry(n).or_default();
            let node = match graph.get_node(n) {
                Some(nd) => nd,
                None => continue,
            };
            for i in 0..node.inputs().len() {
                let in_id = InputSocketId::new(n, i as u32);
                for out in graph.upstream(in_id) {
                    let from = out.node();
                    succ.entry(from).or_default().insert(n);
                }
            }
        }
        succ
    }
}

impl CycleDetector for TarjanSccDetector {
    fn detect(&self, graph: &dyn Graph) -> Option<Vec<NodeId>> {
        let succ = Self::build_successors(graph);
        let mut indices: BTreeMap<NodeId, usize> = BTreeMap::new();
        let mut lowlink: BTreeMap<NodeId, usize> = BTreeMap::new();
        let mut on_stack: BTreeSet<NodeId> = BTreeSet::new();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut index_counter: usize = 0;
        let mut sccs: Vec<Vec<NodeId>> = Vec::new();

        #[allow(clippy::too_many_arguments)]
        fn strongconnect(
            v: NodeId,
            succ: &BTreeMap<NodeId, BTreeSet<NodeId>>,
            indices: &mut BTreeMap<NodeId, usize>,
            lowlink: &mut BTreeMap<NodeId, usize>,
            on_stack: &mut BTreeSet<NodeId>,
            stack: &mut Vec<NodeId>,
            index_counter: &mut usize,
            sccs: &mut Vec<Vec<NodeId>>,
        ) {
            indices.insert(v, *index_counter);
            lowlink.insert(v, *index_counter);
            *index_counter += 1;
            stack.push(v);
            on_stack.insert(v);

            let neighbors: Vec<NodeId> = succ
                .get(&v)
                .map(|set| set.iter().copied().collect())
                .unwrap_or_default();
            for w in neighbors {
                if !indices.contains_key(&w) {
                    strongconnect(
                        w,
                        succ,
                        indices,
                        lowlink,
                        on_stack,
                        stack,
                        index_counter,
                        sccs,
                    );
                    let lw = *lowlink.get(&w).unwrap();
                    let lv = lowlink.get(&v).copied().unwrap();
                    lowlink.insert(v, lv.min(lw));
                } else if on_stack.contains(&w) {
                    let iw = *indices.get(&w).unwrap();
                    let lv = lowlink.get(&v).copied().unwrap();
                    lowlink.insert(v, lv.min(iw));
                }
            }

            if lowlink.get(&v) == indices.get(&v) {
                let mut comp = Vec::new();
                loop {
                    let w = stack.pop().unwrap();
                    on_stack.remove(&w);
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                sccs.push(comp);
            }
        }

        // 根集合排序：遍历起点不随图内存储顺序漂移。
        let mut roots: Vec<NodeId> = graph.all_nodes().collect();
        roots.sort_unstable();
        roots.dedup();
        for r in roots {
            if !indices.contains_key(&r) {
                strongconnect(
                    r,
                    &succ,
                    &mut indices,
                    &mut lowlink,
                    &mut on_stack,
                    &mut stack,
                    &mut index_counter,
                    &mut sccs,
                );
            }
        }

        // An SCC of size > 1 is a cycle; a singleton with a self-loop is too.
        let mut cyclic: Vec<Vec<NodeId>> = Vec::new();
        for mut comp in sccs {
            comp.sort_unstable();
            let is_cycle = comp.len() > 1
                || (comp.len() == 1
                    && succ
                        .get(&comp[0])
                        .map(|set| set.contains(&comp[0]))
                        .unwrap_or(false));
            if is_cycle {
                cyclic.push(comp);
            }
        }
        cyclic.sort_by(|left, right| left[0].cmp(&right[0]));
        cyclic.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use ze_node_core::node::{DirtyPolicy, Node, NodeMeta};
    use ze_node_core::simple_graph::SimpleGraph;
    use ze_node_core::socket::{InputSocket, OutputSocket};
    use ze_node_core::type_tag::NamedTypeTag;

    macro_rules! impl_as_any {
        () => {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        };
    }

    #[derive(Debug)]
    struct N {
        meta: NodeMeta,
        inputs: Vec<InputSocket>,
        outputs: Vec<OutputSocket>,
    }
    impl Node for N {
        impl_as_any!();
        fn meta(&self) -> &NodeMeta {
            &self.meta
        }
        fn inputs(&self) -> &[InputSocket] {
            &self.inputs
        }
        fn outputs(&self) -> &[OutputSocket] {
            &self.outputs
        }
        fn dirty_policy(&self) -> DirtyPolicy {
            DirtyPolicy::Always
        }
        fn state_type(&self) -> Box<dyn ze_node_core::TypeTag> {
            Box::new(NamedTypeTag::new("()"))
        }
    }
    fn mk(n_in: usize, n_out: usize) -> N {
        let tag: Box<dyn ze_node_core::TypeTag> = Box::new(NamedTypeTag::new("f32"));
        N {
            meta: NodeMeta::new("t", "t", 1),
            inputs: (0..n_in)
                .map(|i| InputSocket::new(tag.clone()).with_index(i as u32))
                .collect(),
            outputs: (0..n_out)
                .map(|i| OutputSocket::new(tag.clone()).with_index(i as u32))
                .collect(),
        }
    }

    #[test]
    fn detects_two_node_cycle() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(1, 1)));
        let b = g.add_node(Box::new(mk(1, 1)));
        g.quick_connect(
            ze_node_core::OutputSocketId::new(a, 0),
            ze_node_core::InputSocketId::new(b, 0),
        )
        .unwrap();
        g.quick_connect(
            ze_node_core::OutputSocketId::new(b, 0),
            ze_node_core::InputSocketId::new(a, 0),
        )
        .unwrap();
        let cycle = TarjanSccDetector::new().detect(&g);
        assert!(cycle.is_some());
        let cycle = cycle.unwrap();
        let mut sorted = cycle.clone();
        sorted.sort_unstable();
        assert_eq!(cycle, sorted, "分量内必须升序");
    }

    #[test]
    fn no_cycle_in_dag() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(0, 1)));
        let b = g.add_node(Box::new(mk(1, 0)));
        g.quick_connect(
            ze_node_core::OutputSocketId::new(a, 0),
            ze_node_core::InputSocketId::new(b, 0),
        )
        .unwrap();
        assert!(TarjanSccDetector::new().detect(&g).is_none());
    }

    /// 同一张图跑两次，SCC 输出逐位相同（硬约束 1）。
    #[test]
    fn detection_is_deterministic() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(1, 1)));
        let b = g.add_node(Box::new(mk(1, 1)));
        let c = g.add_node(Box::new(mk(1, 1)));
        let d = g.add_node(Box::new(mk(1, 1)));
        for (from, to) in [(a, b), (b, a), (c, d), (d, c)] {
            g.quick_connect(
                ze_node_core::OutputSocketId::new(from, 0),
                ze_node_core::InputSocketId::new(to, 0),
            )
            .unwrap();
        }
        let detector = TarjanSccDetector::new();
        let first = detector.detect(&g).expect("有环");
        let second = detector.detect(&g).expect("有环");
        assert_eq!(first, second);
        assert_eq!(first[0], first.iter().copied().min().unwrap(), "取最小分量");
    }
}
